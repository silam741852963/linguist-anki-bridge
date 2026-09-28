# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import json
from pathlib import Path
import sqlite3
import stat
import sys
import tempfile
import threading
import unittest
from uuid import uuid4
from concurrent.futures import ThreadPoolExecutor

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.lineage import LineageStore
from linguist_bridge.operations import OperationError, OperationLedger


def request(lineage_id=None, operation_id=None):
    payload = json.dumps({"schema_version": 1, "variant": "create_note", "body": {"fixture": "cat"}},
                         sort_keys=True, separators=(",", ":")).encode()
    return dict(lineage_id=lineage_id or str(uuid4()), operation_id=operation_id or str(uuid4()),
                payload=payload, payload_digest=hashlib.sha256(payload).hexdigest(),
                approved_digest="a" * 64, session_epoch=str(uuid4()),
                owner_token=str(uuid4()), fence_generation=1, variant="create_note")


class OperationLedgerTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name) / "private"
        lineage = LineageStore(self.root)
        self.lineage_id = lineage.lineage("b" * 64, initialize=True)
        lineage.close()

    def tearDown(self):
        self.temporary.cleanup()

    def test_missing_ledger_requires_explicit_initialization_and_restart_keeps_pending(self):
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_LEDGER_MISSING"):
            OperationLedger(self.root)
        ledger = OperationLedger(self.root, initialize=True)
        self.assertEqual(ledger._db.execute("PRAGMA synchronous").fetchone()[0], 2)
        self.assertEqual(ledger._db.execute("PRAGMA journal_mode").fetchone()[0], "wal")
        args = request(self.lineage_id)
        queued = ledger.queue(**args)
        self.assertEqual(queued["state"], "queued")
        self.assertFalse(queued["dispatch_newly_authorized"])
        for candidate in [self.root / "native-operations.sqlite3",
                          self.root / "native-operations.sqlite3-wal"]:
            self.assertEqual(stat.S_IMODE(candidate.stat().st_mode), 0o600)
        self.assertEqual(ledger.queue(**args), queued)
        ledger.close()
        reopened = OperationLedger(self.root)
        self.assertEqual(reopened.status(args["lineage_id"], args["operation_id"]), queued)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            reopened.queue(**request(self.lineage_id))
        reopened.close()

    def test_replay_conflict_fencing_and_unknown_never_dispatch_again(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        ledger.queue(**args)
        forged = dict(args, approved_digest="c" * 64)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_REPLAY_CONFLICT"):
            ledger.queue(**forged)
        forged = dict(args, payload_digest="d" * 64)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PAYLOAD_INVALID"):
            ledger.queue(**forged)
        owner = {key: args[key] for key in ("lineage_id", "operation_id", "owner_token", "fence_generation")}
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_OWNER_CONFLICT"):
            ledger.mark_running(**dict(owner, fence_generation=2))
        started = ledger.mark_running(**owner)
        self.assertTrue(started["transitioned"])
        self.assertEqual(started["status"]["state"], "running")
        self.assertTrue(started["status"]["needs_recovery"])
        self.assertFalse(ledger.mark_running(**owner)["transitioned"])
        ledger.close()
        reopened = OperationLedger(self.root)
        self.assertEqual(reopened.status(args["lineage_id"], args["operation_id"])["state"], "running")
        unknown = reopened.mark_unknown(**owner, reason="transport_ambiguous")
        self.assertEqual(unknown["state"], "unknown")
        self.assertEqual(reopened.mark_unknown(**owner, reason="transport_ambiguous"), unknown)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_REQUIRES_RECOVERY"):
            reopened.mark_running(**owner)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            reopened.queue(**request(self.lineage_id))
        reopened.close()

    def test_concurrent_duplicate_requests_keep_one_event_chain(self):
        ledger = OperationLedger(self.root, initialize=True)
        ledger.close()
        args = request(self.lineage_id)
        barrier = threading.Barrier(3)

        def worker():
            connection = OperationLedger(self.root)
            try:
                barrier.wait(timeout=5)
                return connection.queue(**args)
            finally:
                connection.close()

        with ThreadPoolExecutor(max_workers=2) as pool:
            first = pool.submit(worker)
            second = pool.submit(worker)
            barrier.wait(timeout=5)
            values = [first.result(timeout=5), second.result(timeout=5)]
        self.assertEqual(values[0], values[1])
        ledger = OperationLedger(self.root)
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operations").fetchone()[0], 1)
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operation_events").fetchone()[0], 1)
        ledger.close()

    def test_queued_failure_is_terminal_and_allows_next_request_after_restart(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        ledger.queue(**args)
        owner = {key: args[key] for key in ("lineage_id", "operation_id", "owner_token", "fence_generation")}
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_OWNER_CONFLICT"):
            ledger.fail_before_write(**dict(owner, fence_generation=2), reason="preflight_rejected")
        failed = ledger.fail_before_write(**owner, reason="preflight_rejected")
        self.assertEqual(failed["state"], "failed_before_write")
        self.assertFalse(failed["needs_recovery"])
        self.assertEqual(ledger.fail_before_write(**owner, reason="preflight_rejected"), failed)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.fail_before_write(**owner, reason="operator_cancelled")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.mark_running(**owner)
        ledger.close()
        reopened = OperationLedger(self.root)
        self.assertEqual(reopened.queue(**args), failed)
        second = request(self.lineage_id)
        self.assertEqual(reopened.queue(**second)["state"], "queued")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            reopened.queue(**request(self.lineage_id))
        self.assertEqual(reopened._db.execute("SELECT count(*) FROM operation_events").fetchone()[0], 3)
        reopened.close()

    def test_running_request_cannot_be_failed_before_write(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        ledger.queue(**args)
        owner = {key: args[key] for key in ("lineage_id", "operation_id", "owner_token", "fence_generation")}
        ledger.mark_running(**owner)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.fail_before_write(**owner, reason="preflight_rejected")
        ledger.mark_unknown(**owner, reason="worker_crash")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.fail_before_write(**owner, reason="preflight_rejected")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            ledger.queue(**request(self.lineage_id))
        ledger.close()

    def test_corrupt_closed_history_cannot_unlock_next_request(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        ledger.queue(**args)
        owner = {key: args[key] for key in ("lineage_id", "operation_id", "owner_token", "fence_generation")}
        ledger.fail_before_write(**owner, reason="preflight_rejected")
        ledger._db.execute("DROP TRIGGER operation_events_no_update")
        ledger._db.execute("UPDATE operation_events SET digest='forged' WHERE sequence=2")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_HISTORY_INVALID"):
            ledger.queue(**request(self.lineage_id))
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operations").fetchone()[0], 1)
        ledger.close()

    def test_racing_start_and_before_write_failure_have_one_winner(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        ledger.queue(**args)
        ledger.close()
        owner = {key: args[key] for key in ("lineage_id", "operation_id", "owner_token", "fence_generation")}
        barrier = threading.Barrier(3)

        def worker(start):
            connection = OperationLedger(self.root)
            try:
                barrier.wait(timeout=5)
                try:
                    if start:
                        return connection.mark_running(**owner)["status"]["state"]
                    return connection.fail_before_write(**owner, reason="operator_cancelled")["state"]
                except OperationError as error:
                    return str(error)
            finally:
                connection.close()

        with ThreadPoolExecutor(max_workers=2) as pool:
            started = pool.submit(worker, True)
            failed = pool.submit(worker, False)
            barrier.wait(timeout=5)
            outcomes = {started.result(timeout=5), failed.result(timeout=5)}
        self.assertIn(outcomes, ({"running", "BRIDGE_OPERATION_STATE_CONFLICT"},
                                 {"failed_before_write", "BRIDGE_OPERATION_STATE_CONFLICT"}))
        ledger = OperationLedger(self.root)
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operation_events").fetchone()[0], 2)
        self.assertIn(ledger.status(args["lineage_id"], args["operation_id"])["state"],
                      {"running", "failed_before_write"})
        ledger.close()

    def test_malformed_payload_and_corrupt_or_rewritten_evidence_fail_closed(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        for payload in (b'{"schema_version":1,"variant":"create_note","body":{},"body":{}}',
                        b'{"schema_version":1,"variant":"create_note","body":{},"x":1}',
                        b'{"schema_version":1,"variant":"create_note","body":{"n":NaN}}',
                        b'{"schema_version":1,"variant":"create_note","body":{"n":1e10000}}',
                        b'{"schema_version":1,"variant":"create_note","body":{"s":"\\ud800"}}',
                        b'{"schema_version":1,"variant":"create_note","body":{"nested":' + b'[' * 110 + b'0' + b']' * 110 + b'}}'):
            with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PAYLOAD_INVALID"):
                ledger.queue(**dict(args, payload=payload, payload_digest=hashlib.sha256(payload).hexdigest()))
        ledger.queue(**args)
        with self.assertRaises(sqlite3.DatabaseError):
            ledger._db.execute("UPDATE operation_events SET digest='forged'")
        with self.assertRaises(sqlite3.DatabaseError):
            ledger._db.execute("DELETE FROM operations")
        ledger._db.execute("DROP TRIGGER operation_events_no_update")
        ledger._db.execute("UPDATE operation_events SET digest='forged'")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_HISTORY_INVALID"):
            ledger.status(args["lineage_id"], args["operation_id"])
        ledger.close()

    def test_symlink_and_foreign_installation_are_not_adopted(self):
        ledger = OperationLedger(self.root, initialize=True)
        ledger.close()
        database = self.root / "native-operations.sqlite3"
        foreign = Path(self.temporary.name) / "foreign"
        foreign.write_bytes(b"untouched")
        database.unlink()
        database.symlink_to(foreign)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_FILE_INVALID"):
            OperationLedger(self.root)
        self.assertEqual(foreign.read_bytes(), b"untouched")
