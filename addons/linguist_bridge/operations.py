# SPDX-License-Identifier: GPL-3.0-or-later
"""Durable native operation ledger with owner tokens and monotonic fences.

Rows are append-only: an operation is queued before acceptance, enters
`running` (fsynced) before any native collection effect, and is closed as
`verified` only after an actual read-back. A crash leaves `running`, which a
later owner classifies as `unknown` from worker-liveness evidence; it is
never replayed. Server and collection commits are not one transaction.
"""
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
from uuid import uuid4

from .identity import IdentityError, _read as read_installation_identity
from .payloads import PayloadError, _plan_digest, validate_body
from .protocol import _digest, _uuid


class OperationError(RuntimeError):
    pass


VARIANTS = frozenset({
    "install_model", "export_checkpoint", "store_media", "create_note",
    "update_note", "restore_note", "delete_unstudied_created_note",
})
BEFORE_WRITE_REASONS = frozenset({
    "preflight_rejected", "operator_cancelled", "session_changed", "checkpoint_failed",
    "worker_lost_before_write",
})
UNKNOWN_REASONS = frozenset({
    "transport_ambiguous", "worker_crash", "native_observation_incomplete",
})
IN_FLIGHT = frozenset({"queued", "running"})
SCHEMA_VERSION = 2
MAX_RECEIPT_BYTES = 8192


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


def _approval(value):
    try:
        return _plan_digest(value)
    except PayloadError:
        raise OperationError("BRIDGE_OPERATION_APPROVAL_INVALID") from None


def _fence(value):
    if type(value) is not int or not 1 <= value <= 2**63 - 1:
        raise OperationError("BRIDGE_OPERATION_FENCE_INVALID")
    return value


def _receipt_bytes(receipt):
    if type(receipt) is not dict:
        raise OperationError("BRIDGE_OPERATION_RECEIPT_INVALID")
    try:
        data = json.dumps(receipt, sort_keys=True, separators=(",", ":"),
                          ensure_ascii=False, allow_nan=False).encode()
        _json(data)
    except (TypeError, ValueError):
        raise OperationError("BRIDGE_OPERATION_RECEIPT_INVALID") from None
    if len(data) > MAX_RECEIPT_BYTES:
        raise OperationError("BRIDGE_OPERATION_RECEIPT_INVALID")
    return receipt


SCHEMA = (
    "CREATE TABLE owners(generation INTEGER PRIMARY KEY, token TEXT NOT NULL UNIQUE, lineage_id TEXT NOT NULL, session_epoch TEXT NOT NULL, approved_digest TEXT NOT NULL, issued_ns INTEGER NOT NULL)",
    "CREATE TABLE owner_releases(generation INTEGER PRIMARY KEY REFERENCES owners(generation), released_ns INTEGER NOT NULL)",
)
TABLES_V1 = ("operations", "operation_events")


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
                                       timeout=busy_timeout_ms / 1000, isolation_level=None,
                                       check_same_thread=False)
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
                    self._upgrade(TABLES_V1)
            elif version == 1:
                # The read-only v1 companion never queued operations; only an
                # empty v1 ledger is upgraded in place.
                with self._transaction():
                    if self._db.execute("SELECT count(*) FROM operations").fetchone()[0]:
                        raise OperationError("BRIDGE_OPERATION_SCHEMA_UNSUPPORTED")
                    self._upgrade(())
            elif version != SCHEMA_VERSION:
                raise OperationError("BRIDGE_OPERATION_SCHEMA_UNSUPPORTED")
            if self._db.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
                raise OperationError("BRIDGE_OPERATION_CORRUPT")
            if self._db.execute("SELECT bridge_id FROM installation WHERE singleton=1").fetchone() != (bridge_id,):
                raise OperationError("BRIDGE_OPERATION_INSTALLATION_CONFLICT")
            if self._db.execute("PRAGMA journal_mode=WAL").fetchone()[0] != "wal":
                raise OperationError("BRIDGE_OPERATION_DURABILITY_UNAVAILABLE")
            os.fsync(directory)
            self.bridge_id = bridge_id
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

    def _upgrade(self, immutable_existing):
        for statement in SCHEMA:
            self._db.execute(statement)
        for table in (*immutable_existing, "owners", "owner_releases"):
            self._db.execute(f"CREATE TRIGGER {table}_no_update BEFORE UPDATE ON {table} BEGIN SELECT RAISE(ABORT,'native operation evidence is immutable'); END")
            self._db.execute(f"CREATE TRIGGER {table}_no_delete BEFORE DELETE ON {table} BEGIN SELECT RAISE(ABORT,'native operation evidence is immutable'); END")
        self._db.execute(f"PRAGMA user_version={SCHEMA_VERSION}")

    @contextmanager
    def _transaction(self):
        self._db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self._db.execute("COMMIT")
        except BaseException:
            self._db.execute("ROLLBACK")
            raise

    def _guard(self, function):
        try:
            with self._transaction():
                return function()
        except OperationError:
            raise
        except sqlite3.Error:
            raise OperationError("BRIDGE_OPERATION_IO_FAILED") from None

    # ------------------------------------------------------------ owners
    def _current_owner(self):
        row = self._db.execute(
            "SELECT o.generation,o.token,o.lineage_id,o.session_epoch,o.approved_digest "
            "FROM owners o WHERE o.generation=(SELECT max(generation) FROM owners) "
            "AND NOT EXISTS(SELECT 1 FROM owner_releases r WHERE r.generation=o.generation)").fetchone()
        if row is None:
            return None
        return {"fence": row[0], "owner_token": row[1], "lineage_id": row[2],
                "session_epoch": row[3], "approved_digest": row[4]}

    def current_owner(self):
        return self._guard(self._current_owner)

    def begin_owner(self, *, lineage_id, session_epoch, approved_digest, live_operations):
        """Issue a fresh owner token with the next fence generation.

        `live_operations` is worker-liveness evidence from this process: the
        operation IDs its serialized worker still holds. A `running` row with
        no live worker is classified `unknown(worker_crash)`; a `queued` row
        with no live worker never started its effect and is closed as
        `failed_before_write(worker_lost_before_write)`. Live in-flight work
        blocks a new owner, so a stale fenced request cannot run after owner
        replacement.
        """
        lineage_id, session_epoch = _identifier(lineage_id), _identifier(session_epoch)
        approved_digest = _approval(approved_digest)
        live = frozenset(live_operations)

        def run():
            for prior_lineage, prior_operation in self._db.execute(
                    "SELECT lineage_id,operation_id FROM operations").fetchall():
                status, _ = self._status(prior_lineage, prior_operation)
                if status["state"] not in IN_FLIGHT:
                    continue
                if (prior_lineage, prior_operation) in live:
                    raise OperationError("BRIDGE_OPERATION_PENDING")
                if status["state"] == "running":
                    self._event(prior_lineage, prior_operation, 3, "unknown",
                                status["event_digest"], "worker_crash")
                else:
                    self._event(prior_lineage, prior_operation, 2, "failed_before_write",
                                status["event_digest"], "worker_lost_before_write")
            generation = (self._db.execute("SELECT max(generation) FROM owners").fetchone()[0] or 0) + 1
            token = str(uuid4())
            self._db.execute("INSERT INTO owners VALUES(?,?,?,?,?,?)",
                             (generation, token, lineage_id, session_epoch, approved_digest,
                              time.time_ns()))
            return {"owner_token": token, "fence": generation}
        return self._guard(run)

    def end_owner(self, *, owner_token, fence):
        owner_token, fence = _identifier(owner_token), _fence(fence)

        def run():
            row = self._db.execute("SELECT 1 FROM owners WHERE generation=? AND token=?",
                                   (fence, owner_token)).fetchone()
            if row is None:
                raise OperationError("BRIDGE_OWNER_CONFLICT")
            if self._db.execute("SELECT 1 FROM owner_releases WHERE generation=?",
                                (fence,)).fetchone() is None:
                self._db.execute("INSERT INTO owner_releases VALUES(?,?)", (fence, time.time_ns()))
            return True
        return self._guard(run)

    def _require_owner(self, owner_token, fence, *, lineage_id=None, session_epoch=None,
                       approved_digest=None):
        owner = self._current_owner()
        if owner is None or owner["owner_token"] != owner_token or owner["fence"] != fence:
            raise OperationError("BRIDGE_OWNER_STALE")
        if ((lineage_id is not None and owner["lineage_id"] != lineage_id)
                or (session_epoch is not None and owner["session_epoch"] != session_epoch)
                or (approved_digest is not None and owner["approved_digest"] != approved_digest)):
            raise OperationError("BRIDGE_OWNER_BINDING_CONFLICT")
        return owner

    # ------------------------------------------------------------ operations
    def _status(self, lineage_id, operation_id):
        row = self._db.execute("SELECT payload_digest,approved_digest,session_epoch,owner_token,fence_generation,variant,payload FROM operations WHERE lineage_id=? AND operation_id=?", (lineage_id, operation_id)).fetchone()
        if row is None:
            raise OperationError("BRIDGE_OPERATION_NOT_FOUND")
        if type(row[6]) is not bytes or not 1 <= len(row[6]) <= 1024 * 1024:
            raise OperationError("BRIDGE_OPERATION_RECORD_INVALID")
        try:
            _approval(row[1])
            _uuid(row[2])
            _uuid(row[3])
            decoded = _json(row[6])
        except (ValueError, TypeError, UnicodeError, OperationError):
            raise OperationError("BRIDGE_OPERATION_RECORD_INVALID") from None
        if (hashlib.sha256(row[6]).hexdigest() != row[0]
                or row[5] not in VARIANTS or type(row[4]) is not int or not 1 <= row[4] <= 2**63 - 1
                or type(decoded) is not dict or set(decoded) != {"schema_version", "variant", "body"}
                or type(decoded["schema_version"]) is not int or decoded["schema_version"] != 1
                or decoded["variant"] != row[5]
                or type(decoded["body"]) is not dict):
            raise OperationError("BRIDGE_OPERATION_RECORD_INVALID")
        try:
            validate_body(row[5], decoded["body"], operation_id=operation_id,
                          approved_digest=row[1])
        except PayloadError:
            raise OperationError("BRIDGE_OPERATION_RECORD_INVALID") from None
        events = self._db.execute("SELECT sequence,digest,body FROM operation_events WHERE lineage_id=? AND operation_id=? ORDER BY sequence", (lineage_id, operation_id)).fetchall()
        if not 1 <= len(events) <= 3:
            raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
        previous = None
        state = None
        reason = None
        receipt = None
        for expected, (sequence, digest, body) in enumerate(events, 1):
            if type(body) is not bytes or not 1 <= len(body) <= 10000:
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
            try:
                event = _json(body)
            except (ValueError, TypeError, UnicodeError):
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID") from None
            if (sequence != expected or hashlib.sha256(body).hexdigest() != digest
                    or type(event) is not dict
                    or set(event) != {"sequence", "state", "previous_digest", "reason",
                                      "recorded_ns", "receipt"}
                    or event["sequence"] != expected or event["previous_digest"] != previous
                    or type(event["recorded_ns"]) is not int or event["recorded_ns"] <= 0):
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
            allowed = {1: ("queued",), 2: ("running", "failed_before_write"),
                       3: ("unknown", "verified")}[expected]
            expected_reason = (BEFORE_WRITE_REASONS if event["state"] == "failed_before_write"
                               else UNKNOWN_REASONS if event["state"] == "unknown" else None)
            if (event["state"] not in allowed
                    or (expected == 3 and state != "running")
                    or (expected_reason is None and event["reason"] is not None)
                    or (expected_reason is not None and
                        (type(event["reason"]) is not str or event["reason"] not in expected_reason))
                    or (event["state"] == "verified" and type(event["receipt"]) is not dict)
                    or (event["state"] not in ("verified", "failed_before_write")
                        and event["receipt"] is not None)
                    or (event["receipt"] is not None and type(event["receipt"]) is not dict)):
                raise OperationError("BRIDGE_OPERATION_HISTORY_INVALID")
            state, reason, previous, receipt = event["state"], event["reason"], digest, event["receipt"]
        return {"lineage_id":lineage_id, "operation_id":operation_id,
                "payload_digest":row[0], "approved_digest":row[1], "session_epoch":row[2],
                "variant":row[5], "state":state, "reason":reason, "receipt":receipt,
                "event_digest":previous, "needs_recovery":state in ("running", "unknown"),
                "dispatch_newly_authorized":False}, row

    def status(self, lineage_id, operation_id):
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)
        return self._guard(lambda: self._status(lineage_id, operation_id)[0])

    def status_or_absent(self, lineage_id, operation_id):
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)

        def run():
            try:
                return self._status(lineage_id, operation_id)[0]
            except OperationError as error:
                if str(error) != "BRIDGE_OPERATION_NOT_FOUND":
                    raise
                return {"lineage_id": lineage_id, "operation_id": operation_id, "state": "absent"}
        return self._guard(run)

    def payload(self, lineage_id, operation_id):
        """Stored exact intent bytes and their decoded envelope."""
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)

        def run():
            _, row = self._status(lineage_id, operation_id)
            return row[6], _json(row[6])
        return self._guard(run)

    def _event(self, lineage_id, operation_id, sequence, state, previous, reason=None,
               receipt=None):
        body = json.dumps({"sequence":sequence, "state":state, "previous_digest":previous,
                           "reason":reason, "recorded_ns":time.time_ns(), "receipt":receipt},
                          sort_keys=True, separators=(",", ":"), ensure_ascii=False,
                          allow_nan=False).encode()
        digest = hashlib.sha256(body).hexdigest()
        self._db.execute("INSERT INTO operation_events VALUES(?,?,?,?,?)",
                         (lineage_id, operation_id, sequence, digest, body))
        return digest

    def queue(self, **request):
        return self.queue_new(**request)[0]

    def operation_owner(self, lineage_id, operation_id):
        """(owner_token, fence) the operation was accepted under."""
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)
        return self._guard(lambda: self._status(lineage_id, operation_id)[1][3:5])

    def queue_new(self, *, lineage_id, operation_id, payload, payload_digest,
                  approved_digest, session_epoch, owner_token, fence_generation, variant):
        """Accept one typed intent from the current owner; also report whether
        this call inserted it (only an inserted row may be dispatched).

        A duplicate UUID with the same payload returns the existing state or
        receipt, whoever sends it, and never dispatches again. Any other
        request is refused while earlier work is still queued or running.
        """
        lineage_id, operation_id = _identifier(lineage_id), _identifier(operation_id)
        session_epoch, owner_token = _identifier(session_epoch), _identifier(owner_token)
        payload_digest, approved_digest = _hash(payload_digest), _approval(approved_digest)
        _fence(fence_generation)
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
            validate_body(variant, decoded["body"], operation_id=operation_id,
                          approved_digest=approved_digest)
        except PayloadError as error:
            raise OperationError(str(error)) from None

        def run():
            existing = self._db.execute("SELECT 1 FROM operations WHERE lineage_id=? AND operation_id=?", (lineage_id,operation_id)).fetchone()
            if existing:
                status, row = self._status(lineage_id, operation_id)
                if (row[0], row[1], row[5], row[6]) != (payload_digest, approved_digest, variant, payload):
                    raise OperationError("BRIDGE_OPERATION_REPLAY_CONFLICT")
                return status, False
            self._require_owner(owner_token, fence_generation, lineage_id=lineage_id,
                                session_epoch=session_epoch, approved_digest=approved_digest)
            # Validate every earlier history before accepting another request.
            for prior_lineage, prior_operation in self._db.execute(
                    "SELECT lineage_id,operation_id FROM operations").fetchall():
                prior_status, _ = self._status(prior_lineage, prior_operation)
                if prior_status["state"] in IN_FLIGHT:
                    raise OperationError("BRIDGE_OPERATION_PENDING")
            self._db.execute("INSERT INTO operations VALUES(?,?,?,?,?,?,?,?,?)",
                             (lineage_id, operation_id, payload_digest, approved_digest,
                              session_epoch, owner_token, fence_generation, variant, payload))
            self._event(lineage_id, operation_id, 1, "queued", None)
            return self._status(lineage_id, operation_id)[0], True
        return self._guard(run)

    def _owned(self, lineage_id, operation_id, owner_token, fence_generation):
        status, row = self._status(lineage_id, operation_id)
        if row[3] != owner_token or row[4] != fence_generation:
            raise OperationError("BRIDGE_OPERATION_OWNER_CONFLICT")
        return status

    def mark_running(self, *, lineage_id, operation_id, owner_token, fence_generation):
        """Durable `running` before any native effect; requires the live owner fence."""
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        _fence(fence_generation)

        def run():
            status = self._owned(lineage_id, operation_id, owner_token, fence_generation)
            if status["state"] == "running":
                return {"transitioned":False, "status":status}
            if status["state"] == "failed_before_write":
                raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
            if status["state"] != "queued":
                raise OperationError("BRIDGE_OPERATION_REQUIRES_RECOVERY")
            self._require_owner(owner_token, fence_generation)
            self._event(lineage_id, operation_id, 2, "running", status["event_digest"])
            return {"transitioned":True, "status":self._status(lineage_id, operation_id)[0]}
        return self._guard(run)

    def mark_unknown(self, *, lineage_id, operation_id, owner_token, fence_generation, reason):
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        _fence(fence_generation)
        if type(reason) is not str or reason not in UNKNOWN_REASONS:
            raise OperationError("BRIDGE_OPERATION_REASON_INVALID")

        def run():
            status = self._owned(lineage_id, operation_id, owner_token, fence_generation)
            if status["state"] == "unknown" and status["reason"] == reason:
                return status
            if status["state"] != "running":
                raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
            self._event(lineage_id, operation_id, 3, "unknown", status["event_digest"], reason)
            return self._status(lineage_id, operation_id)[0]
        return self._guard(run)

    def mark_verified(self, *, lineage_id, operation_id, owner_token, fence_generation, receipt):
        """Close `running` only with an actual read-back receipt."""
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        _fence(fence_generation)
        receipt = _receipt_bytes(receipt)

        def run():
            status = self._owned(lineage_id, operation_id, owner_token, fence_generation)
            if status["state"] == "verified" and status["receipt"] == receipt:
                return status
            if status["state"] != "running":
                raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
            self._event(lineage_id, operation_id, 3, "verified", status["event_digest"],
                        receipt=receipt)
            return self._status(lineage_id, operation_id)[0]
        return self._guard(run)

    def fail_before_write(self, *, lineage_id, operation_id, owner_token, fence_generation, reason,
                          detail=None):
        """Close a queued request only; a running request may already have an effect."""
        lineage_id, operation_id, owner_token = _identifier(lineage_id), _identifier(operation_id), _identifier(owner_token)
        _fence(fence_generation)
        if type(reason) is not str or reason not in BEFORE_WRITE_REASONS:
            raise OperationError("BRIDGE_OPERATION_REASON_INVALID")
        if detail is not None:
            detail = _receipt_bytes(detail)

        def run():
            status = self._owned(lineage_id, operation_id, owner_token, fence_generation)
            if status["state"] == "failed_before_write" and status["reason"] == reason:
                return status
            if status["state"] != "queued":
                raise OperationError("BRIDGE_OPERATION_STATE_CONFLICT")
            self._event(lineage_id, operation_id, 2, "failed_before_write", status["event_digest"],
                        reason, receipt=detail)
            return self._status(lineage_id, operation_id)[0]
        return self._guard(run)

    def close(self):
        if self._db is not None:
            self._db.close()
            self._db = None
