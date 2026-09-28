# SPDX-License-Identifier: GPL-3.0-or-later
"""Inactive native operation ledger. It never dispatches an Anki collection effect."""
from contextlib import contextmanager
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import stat
import time

from .identity import IdentityError, _read as read_installation_identity
from .protocol import _digest, _uuid


class OperationError(RuntimeError):
    pass


VARIANTS = frozenset({
    "install_model", "export_checkpoint", "store_media", "create_note",
    "update_note", "restore_note", "delete_unstudied_created_note",
})
BEFORE_WRITE_REASONS = frozenset({
    "preflight_rejected", "operator_cancelled", "session_changed", "checkpoint_failed",
})
UNKNOWN_REASONS = frozenset({
    "transport_ambiguous", "worker_crash", "native_observation_incomplete",
})


def _strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate key")
        result[key] = value
    return result


def _json(data):
    try:
        result = json.loads(data, object_pairs_hook=_strict_object,
                            parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite")))
    except RecursionError:
        raise ValueError("json depth") from None
    pending = [(result, 0)]
    visited = 0
    while pending:
        value, depth = pending.pop()
        visited += 1
        if visited > 100000 or depth > 100:
            raise ValueError("json limits")
        if type(value) is str and any(0xD800 <= ord(char) <= 0xDFFF for char in value):
            raise ValueError("json surrogate")
        if type(value) is float and not math.isfinite(value):
            raise ValueError("nonfinite")
        if type(value) is dict:
            pending.extend((key, depth + 1) for key in value)
            pending.extend((item, depth + 1) for item in value.values())
        elif type(value) is list:
            pending.extend((item, depth + 1) for item in value)
    return result


def _identifier(value):
    try:
        return _uuid(value)
    except ValueError:
        raise OperationError("BRIDGE_OPERATION_ID_INVALID") from None


def _hash(value):
    try:
        return _digest(value)
    except ValueError:
        raise OperationError("BRIDGE_OPERATION_DIGEST_INVALID") from None


class OperationLedger:
    def __init__(self, root, *, initialize=False, busy_timeout_ms=5000):
        self._db = None
        if type(initialize) is not bool or type(busy_timeout_ms) is not int or not 1 <= busy_timeout_ms <= 60000:
            raise OperationError("BRIDGE_OPERATION_OPEN_INPUT_INVALID")
        try:
            root = Path(root)
            if not root.is_absolute() or root.resolve() != root:
                raise OperationError("BRIDGE_OPERATION_PATH_INVALID")
            info = root.lstat()
            if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
                raise OperationError("BRIDGE_OPERATION_DIRECTORY_INVALID")
        except (OSError, ValueError, TypeError, RuntimeError):
            raise OperationError("BRIDGE_OPERATION_PATH_INVALID") from None
        try:
            directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        except OSError:
            raise OperationError("BRIDGE_OPERATION_DIRECTORY_INVALID") from None
        lock = None
        created = False
        try:
            bridge_id = read_installation_identity(directory)
            path = root / "native-operations.sqlite3"
            if initialize:
                lock = os.open(root / ".native-operations-init.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
                lock_info = os.fstat(lock)
                if not stat.S_ISREG(lock_info.st_mode) or lock_info.st_uid != os.getuid() or stat.S_IMODE(lock_info.st_mode) != 0o600:
                    raise OperationError("BRIDGE_OPERATION_LOCK_INVALID")
                deadline = time.monotonic() + busy_timeout_ms / 1000
                while True:
                    try:
                        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        break
                    except BlockingIOError:
                        if time.monotonic() >= deadline:
                            raise OperationError("BRIDGE_OPERATION_LOCK_TIMEOUT") from None
                        time.sleep(min(0.01, max(0, deadline - time.monotonic())))
                try:
                    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                    os.close(descriptor)
                    created = True
                except FileExistsError:
                    pass
            try:
                before = path.lstat()
            except FileNotFoundError:
                raise OperationError("BRIDGE_OPERATION_LEDGER_MISSING") from None
            if not stat.S_ISREG(before.st_mode) or before.st_uid != os.getuid() or stat.S_IMODE(before.st_mode) != 0o600:
                raise OperationError("BRIDGE_OPERATION_FILE_INVALID")
            for suffix in ("-wal", "-shm"):
                candidate = Path(str(path) + suffix)
                try:
                    side = candidate.lstat()
                except FileNotFoundError:
                    continue
                if not stat.S_ISREG(side.st_mode) or side.st_uid != os.getuid() or stat.S_IMODE(side.st_mode) != 0o600:
                    raise OperationError("BRIDGE_OPERATION_FILE_INVALID")
            self._db = sqlite3.connect(path.as_uri() + "?mode=rw", uri=True,
                                       timeout=busy_timeout_ms / 1000, isolation_level=None)
            after = path.lstat()
            if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
                raise OperationError("BRIDGE_OPERATION_FILE_CONFLICT")
            self._db.execute("PRAGMA trusted_schema=OFF")
            self._db.execute("PRAGMA foreign_keys=ON")
            self._db.execute("PRAGMA synchronous=FULL")
            self._db.execute(f"PRAGMA busy_timeout={busy_timeout_ms}")
            version = self._db.execute("PRAGMA user_version").fetchone()[0]
            if created and version == 0:
                with self._transaction():
                    self._db.execute("CREATE TABLE installation(singleton INTEGER PRIMARY KEY CHECK(singleton=1), bridge_id TEXT NOT NULL)")
                    self._db.execute("INSERT INTO installation VALUES(1,?)", (bridge_id,))
                    self._db.execute("CREATE TABLE operations(lineage_id TEXT NOT NULL, operation_id TEXT NOT NULL, payload_digest TEXT NOT NULL, approved_digest TEXT NOT NULL, session_epoch TEXT NOT NULL, owner_token TEXT NOT NULL, fence_generation INTEGER NOT NULL, variant TEXT NOT NULL, payload BLOB NOT NULL, PRIMARY KEY(lineage_id,operation_id))")
                    self._db.execute("CREATE TABLE operation_events(lineage_id TEXT NOT NULL, operation_id TEXT NOT NULL, sequence INTEGER NOT NULL, digest TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(lineage_id,operation_id,sequence), FOREIGN KEY(lineage_id,operation_id) REFERENCES operations(lineage_id,operation_id))")
                    for table in ("operations", "operation_events"):
                        self._db.execute(f"CREATE TRIGGER {table}_no_update BEFORE UPDATE ON {table} BEGIN SELECT RAISE(ABORT,'native operation evidence is immutable'); END")
                        self._db.execute(f"CREATE TRIGGER {table}_no_delete BEFORE DELETE ON {table} BEGIN SELECT RAISE(ABORT,'native operation evidence is immutable'); END")
                    self._db.execute("PRAGMA user_version=1")
            elif version != 1:
                raise OperationError("BRIDGE_OPERATION_SCHEMA_UNSUPPORTED")
            if self._db.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
                raise OperationError("BRIDGE_OPERATION_CORRUPT")
            if self._db.execute("SELECT bridge_id FROM installation WHERE singleton=1").fetchone() != (bridge_id,):
                raise OperationError("BRIDGE_OPERATION_INSTALLATION_CONFLICT")
            if self._db.execute("PRAGMA journal_mode=WAL").fetchone()[0] != "wal":
                raise OperationError("BRIDGE_OPERATION_DURABILITY_UNAVAILABLE")
            os.fsync(directory)
        except OperationError:
            self.close()
            raise
        except IdentityError:
            self.close()
            raise OperationError("BRIDGE_OPERATION_INSTALLATION_INVALID") from None
        except (OSError, sqlite3.Error, ValueError):
            self.close()
            raise OperationError("BRIDGE_OPERATION_OPEN_FAILED") from None
        finally:
            if lock is not None:
                os.close(lock)
            os.close(directory)

    @contextmanager
    def _transaction(self):
        self._db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self._db.execute("COMMIT")
        except BaseException:
            self._db.execute("ROLLBACK")
            raise

    def _status(self, lineage_id, operation_id):
        row = self._db.execute("SELECT payload_digest,approved_digest,session_epoch,owner_token,fence_generation,variant,payload FROM operations WHERE lineage_id=? AND operation_id=?", (lineage_id, operation_id)).fetchone()
        if row is None:
            raise OperationError("BRIDGE_OPERATION_NOT_FOUND")
        try:
            _digest(row[1])
            _uuid(row[2])
            _uuid(row[3])
            decoded = _json(row[6])
        except (ValueError, TypeError, UnicodeError):
            raise OperationError("BRIDGE_OPERATION_RECORD_INVALID") from None
        if (type(row[6]) is not bytes or hashlib.sha256(row[6]).hexdigest() != row[0]
                or row[5] not in VARIANTS or type(row[4]) is not int or not 1 <= row[4] <= 2**63 - 1
                or type(decoded) is not dict or set(decoded) != {"schema_version", "variant", "body"}
                or type(decoded["schema_version"]) is not int or decoded["schema_version"] != 1
                or decoded["variant"] != row[5]
                or type(decoded["body"]) is not dict):
            raise OperationError("BRIDGE_OPERATION_RECORD_INVALID")
        events = self._db.execute("SELECT sequence,digest,body FROM operation_events WHERE lineage_id=? AND operation_id=? ORDER BY sequence", (lineage_id, operation_id)).fetchall()
        if not 1 <= len(events) <= 3:
            raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
        previous = None
        state = None
        reason = None
        for expected, (sequence, digest, body) in enumerate(events, 1):
            try:
                event = _json(body)
            except (ValueError, TypeError, UnicodeError):
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID") from None
            if (sequence != expected or hashlib.sha256(body).hexdigest() != digest
                    or type(event) is not dict or set(event) != {"sequence", "state", "previous_digest", "reason", "recorded_ns"}
                    or event["sequence"] != expected or event["previous_digest"] != previous
                    or type(event["recorded_ns"]) is not int or event["recorded_ns"] <= 0):
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
            allowed = {1: ("queued",), 2: ("running", "failed_before_write"), 3: ("unknown",)}[expected]
            expected_reason = (BEFORE_WRITE_REASONS if event["state"] == "failed_before_write"
                               else UNKNOWN_REASONS if event["state"] == "unknown" else None)
            if (event["state"] not in allowed
                    or (expected == 3 and state != "running")
                    or (expected_reason is None and event["reason"] is not None)
                    or (expected_reason is not None and
                        (type(event["reason"]) is not str or event["reason"] not in expected_reason))):
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
            state, reason, previous = event["state"], event["reason"], digest
        return {"lineage_id":lineage_id, "operation_id":operation_id,
                "payload_digest":row[0], "approved_digest":row[1], "session_epoch":row[2],
                "variant":row[5], "state":state, "reason":reason,
                "event_digest":previous, "needs_recovery":state in ("running", "unknown"),
                "dispatch_newly_authorized":False}, row

    def status(self, lineage_id, operation_id):
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)
        try:
            with self._transaction():
                return self._status(lineage_id, operation_id)[0]
        except OperationError:
            raise
        except sqlite3.Error:
            raise OperationError("BRIDGE_OPERATION_IO_FAILED") from None

    def _event(self, lineage_id, operation_id, sequence, state, previous, reason=None):
        body = json.dumps({"sequence":sequence, "state":state, "previous_digest":previous,
                           "reason":reason, "recorded_ns":time.time_ns()},
                          sort_keys=True, separators=(",", ":")).encode()
        digest = hashlib.sha256(body).hexdigest()
        self._db.execute("INSERT INTO operation_events VALUES(?,?,?,?,?)",
                         (lineage_id, operation_id, sequence, digest, body))
        return digest

    def queue(self, *, lineage_id, operation_id, payload, payload_digest,
              approved_digest, session_epoch, owner_token, fence_generation, variant):
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)
        session_epoch, owner_token = _identifier(session_epoch), _identifier(owner_token)
        payload_digest, approved_digest = _hash(payload_digest), _hash(approved_digest)
        if type(fence_generation) is not int or not 1 <= fence_generation <= 2**63 - 1:
            raise OperationError("BRIDGE_OPERATION_FENCE_INVALID")
        if type(variant) is not str or variant not in VARIANTS or type(payload) is not bytes or not 1 <= len(payload) <= 1024 * 1024:
            raise OperationError("BRIDGE_OPERATION_PAYLOAD_INVALID")
        try:
            decoded = _json(payload)
        except (ValueError, TypeError, UnicodeError):
            raise OperationError("BRIDGE_OPERATION_PAYLOAD_INVALID") from None
        if (type(decoded) is not dict or set(decoded) != {"schema_version", "variant", "body"}
                or type(decoded["schema_version"]) is not int or decoded["schema_version"] != 1
                or decoded["variant"] != variant or type(decoded["body"]) is not dict
                or hashlib.sha256(payload).hexdigest() != payload_digest):
            raise OperationError("BRIDGE_OPERATION_PAYLOAD_INVALID")
        try:
            with self._transaction():
                existing = self._db.execute("SELECT 1 FROM operations WHERE lineage_id=? AND operation_id=?", (lineage_id,operation_id)).fetchone()
                expected = (payload_digest, approved_digest, session_epoch, owner_token, fence_generation, variant, payload)
                if existing:
                    status, row = self._status(lineage_id, operation_id)
                    if row != expected:
                        raise OperationError("BRIDGE_OPERATION_REPLAY_CONFLICT")
                    return status
                # Validate all earlier histories before allowing another request.
                # Only an operation that never entered running may be superseded.
                for prior_lineage, prior_operation in self._db.execute(
                        "SELECT lineage_id,operation_id FROM operations"):
                    prior_status, _ = self._status(prior_lineage, prior_operation)
                    if prior_status["state"] != "failed_before_write":
                        raise OperationError("BRIDGE_OPERATION_PENDING")
                self._db.execute("INSERT INTO operations VALUES(?,?,?,?,?,?,?,?,?)",
                                 (lineage_id, operation_id, *expected))
                self._event(lineage_id, operation_id, 1, "queued", None)
                return self._status(lineage_id, operation_id)[0]
        except OperationError:
            raise
        except sqlite3.Error:
            raise OperationError("BRIDGE_OPERATION_IO_FAILED") from None

    def mark_running(self, *, lineage_id, operation_id, owner_token, fence_generation):
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        if type(fence_generation) is not int or not 1 <= fence_generation <= 2**63 - 1:
            raise OperationError("BRIDGE_OPERATION_FENCE_INVALID")
        try:
            with self._transaction():
                status, row = self._status(lineage_id, operation_id)
                if row[3] != owner_token or row[4] != fence_generation:
                    raise OperationError("BRIDGE_OPERATION_OWNER_CONFLICT")
                if status["state"] == "running":
                    return {"transitioned":False, "status":status}
                if status["state"] == "failed_before_write":
                    raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
                if status["state"] != "queued":
                    raise OperationError("BRIDGE_OPERATION_REQUIRES_RECOVERY")
                self._event(lineage_id, operation_id, 2, "running", status["event_digest"])
                return {"transitioned":True, "status":self._status(lineage_id, operation_id)[0]}
        except OperationError:
            raise
        except sqlite3.Error:
            raise OperationError("BRIDGE_OPERATION_IO_FAILED") from None

    def mark_unknown(self, *, lineage_id, operation_id, owner_token, fence_generation, reason):
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        if type(fence_generation) is not int or not 1 <= fence_generation <= 2**63 - 1:
            raise OperationError("BRIDGE_OPERATION_FENCE_INVALID")
        if type(reason) is not str or reason not in UNKNOWN_REASONS:
            raise OperationError("BRIDGE_OPERATION_REASON_INVALID")
        try:
            with self._transaction():
                status, row = self._status(lineage_id, operation_id)
                if row[3] != owner_token or row[4] != fence_generation:
                    raise OperationError("BRIDGE_OPERATION_OWNER_CONFLICT")
                if status["state"] == "unknown" and status["reason"] == reason:
                    return status
                if status["state"] != "running":
                    raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
                self._event(lineage_id, operation_id, 3, "unknown", status["event_digest"], reason)
                return self._status(lineage_id, operation_id)[0]
        except OperationError:
            raise
        except sqlite3.Error:
            raise OperationError("BRIDGE_OPERATION_IO_FAILED") from None

    def fail_before_write(self, *, lineage_id, operation_id, owner_token, fence_generation, reason):
        """Close a queued request only; a running request may already have an effect."""
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        if type(fence_generation) is not int or not 1 <= fence_generation <= 2**63 - 1:
            raise OperationError("BRIDGE_OPERATION_FENCE_INVALID")
        if type(reason) is not str or reason not in BEFORE_WRITE_REASONS:
            raise OperationError("BRIDGE_OPERATION_REASON_INVALID")
        try:
            with self._transaction():
                status, row = self._status(lineage_id, operation_id)
                if row[3] != owner_token or row[4] != fence_generation:
                    raise OperationError("BRIDGE_OPERATION_OWNER_CONFLICT")
                if status["state"] == "failed_before_write" and status["reason"] == reason:
                    return status
                if status["state"] != "queued":
                    raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
                self._event(lineage_id, operation_id, 2, "failed_before_write", status["event_digest"], reason)
                return self._status(lineage_id, operation_id)[0]
        except OperationError:
            raise
        except sqlite3.Error:
            raise OperationError("BRIDGE_OPERATION_IO_FAILED") from None

    def close(self):
        if self._db is not None:
            self._db.close()
            self._db = None
