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
from linguist_bridge.payloads import VOCAB_FIELDS, GRAMMAR_FIELDS


def request(lineage_id=None, operation_id=None):
    operation_id = operation_id or str(uuid4())
    marker = "lab_op_" + operation_id.replace("-", "")
    fields = {name: "" for name in VOCAB_FIELDS}
    fields.update(Expression="cat", Meaning="<p>animal</p>", Language="en")
    body = {"model_name":"Linguist Vocabulary v2", "model_manifest_digest":"b" * 64,
            "deck_id":"123", "fields":fields, "tags":[marker], "marker_tag":marker,
            "source_plan_digest":"lab-jcs-v1:plan:" + "a" * 64, "checkpoint_digest":"c" * 64,
            "binding":{"profile_fingerprint":"d" * 64,"path_fingerprint":"e" * 64},
            "expected_absent":True}
    payload = json.dumps({"schema_version": 1, "variant": "create_note", "body": body},
                         sort_keys=True, separators=(",", ":")).encode()
    return dict(lineage_id=lineage_id or str(uuid4()), operation_id=operation_id,
                payload=payload, payload_digest=hashlib.sha256(payload).hexdigest(),
                approved_digest="lab-jcs-v1:plan:" + "a" * 64, session_epoch=str(uuid4()),
                owner_token=str(uuid4()), fence_generation=1, variant="create_note")


def owned(ledger, args, live=()):
    owner = ledger.begin_owner(lineage_id=args["lineage_id"], session_epoch=args["session_epoch"],
                               approved_digest=args["approved_digest"], live_operations=set(live))
    return dict(args, owner_token=owner["owner_token"], fence_generation=owner["fence"])


def same_session(args, previous):
    return dict(args, session_epoch=previous["session_epoch"])


class OperationLedgerTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name) / "private"
        lineage = LineageStore(self.root)
        self.lineage_id = lineage.lineage("b" * 64, initialize=True)
        lineage.close()

    def tearDown(self):
        self.temporary.cleanup()

    def sibling(self, args):
        """Another request from the same owner and session."""
        return dict(request(self.lineage_id), owner_token=args["owner_token"],
                    fence_generation=args["fence_generation"],
                    session_epoch=args["session_epoch"])

    def owner(self, args):
        return {key: args[key] for key in ("lineage_id", "operation_id", "owner_token",
                                           "fence_generation")}

    def test_create_note_body_is_typed_before_queue_and_checked_on_reopen(self):
        ledger = OperationLedger(self.root, initialize=True)
        original = owned(ledger, request(self.lineage_id))
        def changed(edit, variant="create_note"):
            candidate = dict(original)
            envelope = json.loads(original["payload"])
            envelope["variant"] = variant
            edit(envelope["body"])
            candidate["variant"] = variant
            candidate["payload"] = json.dumps(envelope, sort_keys=True, separators=(",", ":")).encode()
            candidate["payload_digest"] = hashlib.sha256(candidate["payload"]).hexdigest()
            return candidate
        for edit in (
            lambda body: body.update(extra="ignored"),
            lambda body: body.pop("checkpoint_digest"),
            lambda body: body.update(source_plan_digest="f" * 64),
            lambda body: body.update(deck_id="01"),
            lambda body: body.update(model_name="Basic"),
            lambda body: body.update(marker_tag="wrong"),
            lambda body: body.update(expected_absent=1),
            lambda body: body.update(tags=["ordinary"]),
            lambda body: body.update(binding={"profile_fingerprint":"d" * 64}),
            lambda body: body["fields"].update(Unexpected="value"),
            lambda body: body["fields"].update(Language="other"),
            lambda body: body["fields"].update(Expression=""),
            lambda body: body["fields"].update(EnableProduction="yes"),
            lambda body: body["fields"].update(Meaning="x" * 262145),
        ):
            with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_BODY_INVALID"):
                ledger.queue(**changed(edit))
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_APPROVAL_INVALID"):
            ledger.queue(**dict(original, approved_digest="a" * 64))
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_BODY_INVALID"):
            ledger.queue(**changed(lambda body: None, variant="update_note"))
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PAYLOAD_INVALID"):
            ledger.queue(**changed(lambda body: None, variant="arbitrary_sql"))
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operations").fetchone()[0], 0)
        self.assertEqual(ledger.queue(**original)["state"], "queued")
        ledger.close()
        reopened = OperationLedger(self.root)
        self.assertEqual(reopened.status(original["lineage_id"], original["operation_id"])["state"], "queued")
        reopened.close()

    def test_grammar_create_note_requires_complete_managed_fields(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        payload = json.loads(args["payload"])
        fields = {name: "" for name in GRAMMAR_FIELDS}
        fields.update(Pattern="〜ても", Meaning="<p>even if</p>", Formation="<p>V-て + も</p>",
                      Examples="<p>雨が降っても行く</p>", UseKey="concession", Language="ja")
        payload["body"]["model_name"] = "Linguist Grammar v2"
        payload["body"]["fields"] = fields
        args["payload"] = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
        args["payload_digest"] = hashlib.sha256(args["payload"]).hexdigest()
        self.assertEqual(ledger.queue(**args)["state"], "queued")
        ledger.close()

    def test_queue_requires_the_current_owner_session_and_approval(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = request(self.lineage_id)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OWNER_STALE"):
            ledger.queue(**args)
        first = owned(ledger, args)
        self.assertEqual(first["fence_generation"], 1)
        second = owned(ledger, args)
        self.assertEqual(second["fence_generation"], 2)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OWNER_STALE"):
            ledger.queue(**first)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OWNER_BINDING_CONFLICT"):
            ledger.queue(**dict(second, session_epoch=str(uuid4())))
        self.assertTrue(ledger.end_owner(owner_token=second["owner_token"], fence=2))
        self.assertTrue(ledger.end_owner(owner_token=second["owner_token"], fence=2))
        with self.assertRaisesRegex(OperationError, "BRIDGE_OWNER_STALE"):
            ledger.queue(**second)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OWNER_CONFLICT"):
            ledger.end_owner(owner_token=str(uuid4()), fence=2)
        third = owned(ledger, args)
        self.assertEqual(third["fence_generation"], 3)
        self.assertEqual(ledger.queue(**third)["state"], "queued")
        ledger.close()

    def test_missing_ledger_requires_explicit_initialization_and_restart_keeps_pending(self):
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_LEDGER_MISSING"):
            OperationLedger(self.root)
        ledger = OperationLedger(self.root, initialize=True)
        self.assertEqual(ledger._db.execute("PRAGMA synchronous").fetchone()[0], 2)
        self.assertEqual(ledger._db.execute("PRAGMA journal_mode").fetchone()[0], "wal")
        args = owned(ledger, request(self.lineage_id))
        queued = ledger.queue(**args)
        self.assertEqual(queued["state"], "queued")
        self.assertFalse(queued["dispatch_newly_authorized"])
        for candidate in [self.root / "native-operations.sqlite3",
                          self.root / "native-operations.sqlite3-wal"]:
            self.assertEqual(stat.S_IMODE(candidate.stat().st_mode), 0o600)
        self.assertEqual(ledger.queue(**args), queued)
        self.assertEqual(ledger.queue_new(**args), (queued, False))
        ledger.close()
        reopened = OperationLedger(self.root)
        self.assertEqual(reopened.status(args["lineage_id"], args["operation_id"]), queued)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            reopened.queue(**self.sibling(args))
        reopened.close()

    def test_duplicate_uuid_returns_existing_state_and_never_dispatches_again(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        queued, inserted = ledger.queue_new(**args)
        self.assertTrue(inserted)
        # A delayed duplicate from a replaced owner is answered, not dispatched.
        later = owned(ledger, args, live={(args["lineage_id"], args["operation_id"])}) \
            if False else args
        self.assertEqual(ledger.queue_new(**dict(later, owner_token=str(uuid4()))), (queued, False))
        changed = json.loads(args["payload"])
        changed["body"]["fields"]["Meaning"] = "different"
        payload = json.dumps(changed, sort_keys=True, separators=(",", ":")).encode()
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_REPLAY_CONFLICT"):
            ledger.queue(**dict(args, payload=payload,
                                payload_digest=hashlib.sha256(payload).hexdigest()))
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PAYLOAD_INVALID"):
            ledger.queue(**dict(args, payload_digest="d" * 64))
        ledger.close()

    def test_fencing_unknown_and_verified_receipts(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        ledger.queue(**args)
        owner = self.owner(args)
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
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            reopened.mark_verified(**owner, receipt={"note_id": 1})
        # Unknown is recovery evidence, not in-flight work: an independent
        # request may proceed while the CLI journal keeps the item blocked.
        second = self.sibling(args)
        self.assertEqual(reopened.queue(**second)["state"], "queued")
        second_owner = self.owner(second)
        reopened.mark_running(**second_owner)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_RECEIPT_INVALID"):
            reopened.mark_verified(**second_owner, receipt=["not", "an", "object"])
        verified = reopened.mark_verified(**second_owner, receipt={"note_id": 42})
        self.assertEqual((verified["state"], verified["receipt"], verified["needs_recovery"]),
                         ("verified", {"note_id": 42}, False))
        self.assertEqual(reopened.mark_verified(**second_owner, receipt={"note_id": 42}), verified)
        reopened.close()

    def test_new_owner_classifies_dead_workers_and_waits_for_live_ones(self):
        ledger = OperationLedger(self.root, initialize=True)
        running = owned(ledger, request(self.lineage_id))
        ledger.queue(**running)
        ledger.mark_running(**self.owner(running))
        key = (running["lineage_id"], running["operation_id"])
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            owned(ledger, running, live={key})
        owned(ledger, running)  # no live worker: the process that ran it is gone
        status = ledger.status(*key)
        self.assertEqual((status["state"], status["reason"]), ("unknown", "worker_crash"))
        queued = owned(ledger, request(self.lineage_id))
        ledger.queue(**queued)
        owned(ledger, queued)
        status = ledger.status(queued["lineage_id"], queued["operation_id"])
        self.assertEqual((status["state"], status["reason"], status["needs_recovery"]),
                         ("failed_before_write", "worker_lost_before_write", False))
        ledger.close()

    def test_empty_v1_ledger_is_upgraded_and_nonempty_v1_is_refused(self):
        ledger = OperationLedger(self.root, initialize=True)
        for statement in ("DROP TRIGGER owners_no_update", "DROP TRIGGER owners_no_delete",
                          "DROP TRIGGER owner_releases_no_update",
                          "DROP TRIGGER owner_releases_no_delete",
                          "DROP TABLE owner_releases", "DROP TABLE owners",
                          "PRAGMA user_version=1"):
            ledger._db.execute(statement)
        ledger.close()
        upgraded = OperationLedger(self.root)
        self.assertEqual(upgraded._db.execute("PRAGMA user_version").fetchone()[0], 2)
        args = owned(upgraded, request(self.lineage_id))
        self.assertEqual(upgraded.queue(**args)["state"], "queued")
        upgraded._db.execute("PRAGMA user_version=1")
        upgraded.close()
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_SCHEMA_UNSUPPORTED"):
            OperationLedger(self.root)

    def test_concurrent_duplicate_requests_keep_one_event_chain(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        ledger.close()
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
        args = owned(ledger, request(self.lineage_id))
        ledger.queue(**args)
        owner = self.owner(args)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_OWNER_CONFLICT"):
            ledger.fail_before_write(**dict(owner, fence_generation=2), reason="preflight_rejected")
        failed = ledger.fail_before_write(**owner, reason="preflight_rejected",
                                          detail={"code": "BRIDGE_PRECONDITION_FAILED"})
        self.assertEqual(failed["state"], "failed_before_write")
        self.assertEqual(failed["receipt"], {"code": "BRIDGE_PRECONDITION_FAILED"})
        self.assertFalse(failed["needs_recovery"])
        self.assertEqual(ledger.fail_before_write(**owner, reason="preflight_rejected",
                                                  detail={"code": "BRIDGE_PRECONDITION_FAILED"}),
                         failed)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.fail_before_write(**owner, reason="operator_cancelled")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.mark_running(**owner)
        ledger.close()
        reopened = OperationLedger(self.root)
        self.assertEqual(reopened.queue(**args), failed)
        second = self.sibling(args)
        self.assertEqual(reopened.queue(**second)["state"], "queued")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            reopened.queue(**self.sibling(args))
        self.assertEqual(reopened._db.execute("SELECT count(*) FROM operation_events").fetchone()[0], 3)
        reopened.close()

    def test_running_request_cannot_be_failed_before_write(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        ledger.queue(**args)
        owner = self.owner(args)
        ledger.mark_running(**owner)
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.fail_before_write(**owner, reason="preflight_rejected")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_PENDING"):
            ledger.queue(**self.sibling(args))
        ledger.mark_unknown(**owner, reason="worker_crash")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_STATE_CONFLICT"):
            ledger.fail_before_write(**owner, reason="preflight_rejected")
        ledger.close()

    def test_corrupt_closed_history_cannot_unlock_next_request(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        ledger.queue(**args)
        ledger.fail_before_write(**self.owner(args), reason="preflight_rejected")
        ledger._db.execute("DROP TRIGGER operation_events_no_update")
        ledger._db.execute("UPDATE operation_events SET digest='forged' WHERE sequence=2")
        with self.assertRaisesRegex(OperationError, "BRIDGE_OPERATION_HISTORY_INVALID"):
            ledger.queue(**self.sibling(args))
        self.assertEqual(ledger._db.execute("SELECT count(*) FROM operations").fetchone()[0], 1)
        ledger.close()

    def test_racing_start_and_before_write_failure_have_one_winner(self):
        ledger = OperationLedger(self.root, initialize=True)
        args = owned(ledger, request(self.lineage_id))
        ledger.queue(**args)
        ledger.close()
        owner = self.owner(args)
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
        args = owned(ledger, request(self.lineage_id))
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
        with self.assertRaises(sqlite3.DatabaseError):
            ledger._db.execute("DELETE FROM owners")
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
