# SPDX-License-Identifier: GPL-3.0-or-later
"""lab-native-v1 control and mutation actions (WP-03).

`labBegin`, `labEnd`, `labInspect`, `labMutate`, `labRebind` and the read
actions run inside the existing AnkiConnect listener on Anki's main thread.
Controls require a configured AnkiConnect API key (the listener rejects a
request whose key differs). A mutation is accepted promptly as `queued` and
executed later in one brief, non-reentrant main-thread critical callback
while the collection executor slot is held, so no other collection operation
or UI/AnkiConnect timer write can interleave between the precondition check,
the durable `running` record and the native calls. The CLI polls
`labOperationStatus`; only an actual read-back closes an operation as
`verified`.
"""
import base64
import errno
import hashlib
import json
import os
from pathlib import Path
import stat
import threading
import time

from . import effects
from . import manifest as canonical_manifest
from .operations import OperationError
from .payloads import media_name, PayloadError
from .protocol import _uuid
from .session import SessionError

MUTATE_KEYS = frozenset({"lineage_id", "operation_id", "session_epoch", "owner_token",
                         "fence", "approved_digest", "variant", "payload"})
INSPECT_KINDS = frozenset({"note", "notes_tagged", "models_named", "deck", "media",
                           "media_bytes", "scope", "note_evidence"})
MAX_MEDIA_READ = 64 * 1024 * 1024
CRITICAL_WAIT_SECONDS = 120


class NativeError(RuntimeError):
    pass


class FaultInjector:
    """Disposable-test fault injection; inert unless the environment names a file.

    The file holds one JSON object `{"point", "action", "seconds"?, "variant"?}`.
    It is consumed (renamed) the first time its point is reached, so a fault
    fires once. Points: `before_running`, `after_effect`, `before_export`.
    Actions: `crash` (immediate process exit), `disk_full` (ENOSPC at the
    ledger boundary), `sleep` (delay the critical section).
    """

    def __init__(self, path=None):
        self._path = Path(path) if path else None

    @classmethod
    def from_environment(cls):
        return cls(os.environ.get("LINGUIST_BRIDGE_FAULT_FILE") or None)

    def take(self, point, variant):
        if self._path is None:
            return None
        try:
            with open(self._path, "rb") as handle:
                fault = json.loads(handle.read(4096))
        except (OSError, ValueError):
            return None
        if (type(fault) is not dict or fault.get("point") != point
                or fault.get("variant", variant) != variant):
            return None
        try:
            os.replace(self._path, str(self._path) + ".consumed")
        except OSError:
            return None
        return fault

    def fire(self, point, variant):
        fault = self.take(point, variant)
        if fault is None:
            return
        action = fault.get("action")
        if action == "crash":
            os._exit(70)
        if action == "disk_full":
            raise OSError(errno.ENOSPC, "No space left on device (injected)")
        if action == "sleep":
            time.sleep(min(float(fault.get("seconds", 1)), 600))


class AnkiScheduler:
    """Serialization through Anki's collection executor (max one worker)."""

    def __init__(self, main_window):
        self._mw = main_window

    def critical(self, function):
        """Run `function` on the main thread while this worker holds the
        collection executor slot; no other collection operation runs meanwhile."""
        taskman = self._mw.taskman
        state = {"started": False, "abandoned": False}
        lock = threading.Lock()
        done = threading.Event()

        def on_main():
            with lock:
                if state["abandoned"]:
                    return
                state["started"] = True
            try:
                function()
            finally:
                done.set()

        def worker():
            taskman.run_on_main(on_main)
            if not done.wait(CRITICAL_WAIT_SECONDS):
                with lock:
                    if not state["started"]:
                        state["abandoned"] = True
                        return
                done.wait()

        taskman.run_in_background(worker, None, uses_collection=True)

    def background(self, task, on_done):
        def finished(future):
            try:
                on_done(future.result(), None)
            except Exception as error:  # noqa: BLE001 - reported as unknown
                on_done(None, error)
        self._mw.taskman.run_in_background(task, finished, uses_collection=True)


def _require(condition, code):
    if not condition:
        raise NativeError(code)


class NativeActions:
    """The action bodies; `runtime` supplies session, ledger and collection."""

    def __init__(self, runtime, *, scheduler, faults, api_key):
        self._runtime = runtime
        self._scheduler = scheduler
        self._faults = faults
        self._api_key = api_key
        self._live = set()
        self._lock = threading.Lock()

    # ------------------------------------------------------------ helpers
    def _authenticated(self):
        key = self._api_key()
        _require(type(key) is str and key.strip(), "BRIDGE_AUTH_REQUIRED")

    def _session(self, expected_epoch=None):
        try:
            session = self._runtime.observed_session()
        except (SessionError, AttributeError):
            raise NativeError("BRIDGE_SESSION_UNAVAILABLE") from None
        if expected_epoch is not None and session["session_epoch"] != expected_epoch:
            raise NativeError("BRIDGE_SESSION_CONFLICT")
        return session

    def _col(self):
        col = self._runtime.collection()
        _require(col is not None, "BRIDGE_COLLECTION_UNAVAILABLE")
        return col

    def live_operations(self):
        with self._lock:
            return set(self._live)

    # ------------------------------------------------------------ controls
    def begin(self, binding, approved_digest):
        self._authenticated()
        _require(type(binding) is dict, "BRIDGE_BINDING_INVALID")
        session = self._session(binding.get("session_epoch"))
        expected = {"lineage_id": session["lineage_id"],
                    "profile_fingerprint": session["profile_fingerprint"],
                    "path_fingerprint": session["path_fingerprint"],
                    "bridge_id": self._runtime.bridge_id}
        if any(binding.get(key) != value for key, value in expected.items()):
            raise NativeError("BRIDGE_BINDING_MISMATCH")
        try:
            owner = self._runtime.ledger.begin_owner(
                lineage_id=session["lineage_id"], session_epoch=session["session_epoch"],
                approved_digest=approved_digest,
                live_operations=self.live_operations())
        except OperationError as error:
            raise NativeError(str(error)) from None
        return dict(owner, staging_dir=str(self._runtime.staging_dir))

    def end(self, owner_token, fence):
        self._authenticated()
        try:
            return self._runtime.ledger.end_owner(owner_token=owner_token, fence=fence)
        except OperationError as error:
            raise NativeError(str(error)) from None

    def rebind(self, lineage_id, previous_epoch):
        """Explicit continuation evidence after a session change. No mutation."""
        self._authenticated()
        session = self._session()
        _require(session["lineage_id"] == lineage_id, "BRIDGE_LINEAGE_MISMATCH")
        return {"collection_session": session,
                "previous_epoch_current": session["session_epoch"] == previous_epoch,
                "in_flight": sorted(op for _, op in self.live_operations())}

    def status(self, lineage_id, operation_id):
        try:
            return self._runtime.ledger.status_or_absent(lineage_id, operation_id)
        except OperationError as error:
            raise NativeError(str(error)) from None

    # ------------------------------------------------------------ reads
    def inspect(self, params):
        if type(params) is not dict or params.get("kind") not in INSPECT_KINDS:
            raise NativeError("BRIDGE_INSPECT_INVALID")
        self._session(params.get("session_epoch"))
        col = self._col()
        kind = params["kind"]
        if kind == "note":
            return effects.observe_or_none(col, _wire(params.get("note_id")))
        if kind == "notes_tagged":
            tag = params.get("tag")
            return effects.notes_tagged(col, _tag(tag))
        if kind == "models_named":
            return effects.models_named(col, _text(params.get("name")))
        if kind == "deck":
            return effects.deck_named(col, _text(params.get("name")))
        if kind == "media":
            return effects.media_observation(col, _media(params.get("filename")))
        if kind == "media_bytes":
            return self._media_bytes(col, _media(params.get("filename")),
                                     params.get("max_bytes"))
        if kind == "scope":
            notes = params.get("note_ids")
            models = params.get("model_ids", [])
            requirement = params.get("requirement")
            _require(type(notes) is list and len(notes) <= 100000
                     and type(models) is list and len(models) <= 1000
                     and type(requirement) is dict
                     and set(requirement) == {"scheduling", "media", "schema"}
                     and all(type(v) is bool for v in requirement.values()),
                     "BRIDGE_INSPECT_INVALID")
            try:
                return effects.scope_manifest(col, [_wire(n) for n in notes], requirement,
                                              [_wire(m) for m in models])
            except effects.EffectError as error:
                raise NativeError(str(error)) from None
        return self._runtime.inspect_note(str(_wire(params.get("note_id"))),
                                          params.get("session_epoch"))

    def _media_bytes(self, col, name, limit):
        _require(type(limit) is int and 0 < limit <= MAX_MEDIA_READ, "BRIDGE_INSPECT_INVALID")
        directory = os.open(col.media.dir(), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            try:
                handle = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=directory)
            except FileNotFoundError:
                return None
            try:
                info = os.fstat(handle)
                _require(stat.S_ISREG(info.st_mode), "BRIDGE_MEDIA_FILE_INVALID")
                _require(info.st_size <= limit, "BRIDGE_MEDIA_TOO_LARGE")
                data = os.read(handle, limit + 1)
                _require(len(data) == info.st_size, "BRIDGE_MEDIA_CHANGED")
            finally:
                os.close(handle)
        finally:
            os.close(directory)
        return base64.b64encode(data).decode("ascii")

    # ------------------------------------------------------------ mutation
    def mutate(self, params):
        self._authenticated()
        if type(params) is not dict or set(params) != MUTATE_KEYS:
            raise NativeError("BRIDGE_MUTATE_INVALID")
        payload = params["payload"]
        _require(type(payload) is str and 0 < len(payload) <= 1024 * 1024,
                 "BRIDGE_MUTATE_INVALID")
        raw = payload.encode("utf-8")
        session = self._session(params["session_epoch"])
        _require(session["lineage_id"] == params["lineage_id"], "BRIDGE_LINEAGE_MISMATCH")
        try:
            status, inserted = self._runtime.ledger.queue_new(
                lineage_id=params["lineage_id"], operation_id=params["operation_id"],
                payload=raw, payload_digest=hashlib.sha256(raw).hexdigest(),
                approved_digest=params["approved_digest"],
                session_epoch=params["session_epoch"], owner_token=params["owner_token"],
                fence_generation=params["fence"], variant=params["variant"])
        except OperationError as error:
            raise NativeError(str(error)) from None
        if inserted:
            key = (status["lineage_id"], status["operation_id"])
            with self._lock:
                self._live.add(key)
            try:
                self._scheduler.critical(lambda: self._execute(key))
            except BaseException:
                with self._lock:
                    self._live.discard(key)
                raise
        return status

    def _finish(self, key):
        with self._lock:
            self._live.discard(key)

    def _execute(self, key):
        """Main-thread critical section for one queued operation."""
        lineage_id, operation_id = key
        ledger = self._runtime.ledger
        defer_finish = False
        try:
            _, envelope = ledger.payload(lineage_id, operation_id)
            status = ledger.status(lineage_id, operation_id)
            if status["state"] != "queued":
                return
            owner = {"lineage_id": lineage_id, "operation_id": operation_id,
                     "owner_token": None, "fence_generation": None}
            row_owner = ledger.operation_owner(lineage_id, operation_id)
            owner.update(owner_token=row_owner[0], fence_generation=row_owner[1])
            variant = envelope["variant"]
            body = envelope["body"]
            current = ledger.current_owner()
            if (current is None or current["owner_token"] != owner["owner_token"]
                    or current["fence"] != owner["fence_generation"]):
                ledger.fail_before_write(**owner, reason="operator_cancelled",
                                         detail={"code": "BRIDGE_OWNER_STALE"})
                return
            try:
                session = self._session(status["session_epoch"])
                _require(session["lineage_id"] == lineage_id, "BRIDGE_LINEAGE_MISMATCH")
            except NativeError as error:
                ledger.fail_before_write(**owner, reason="session_changed",
                                         detail={"code": str(error)})
                return
            col = self._col()
            try:
                prepared = self._preflight(col, variant, body)
            except (effects.EffectError, NativeError, PayloadError) as error:
                ledger.fail_before_write(**owner, reason="preflight_rejected",
                                         detail={"code": _code(error)})
                return
            self._faults.fire("before_running", variant)
            ledger.mark_running(**owner)
            if variant == "export_checkpoint":
                defer_finish = True
                self._export(key, owner, body)
                return
            try:
                result = self._perform(col, variant, body, prepared)
                self._faults.fire("after_effect", variant)
                receipt = self._readback(col, variant, body, result)
            except Exception:  # noqa: BLE001 - an effect may have happened
                ledger.mark_unknown(**owner, reason="native_observation_incomplete")
                return
            ledger.mark_verified(**owner, receipt=receipt)
        except (OperationError, OSError):
            # A ledger write failed (for example ENOSPC). The row keeps its
            # last durable state; `labBegin` later classifies it by liveness.
            return
        finally:
            if not defer_finish:
                self._finish(key)

    def _preflight(self, col, variant, body):
        if variant == "create_note":
            return effects.preflight_create_note(col, body)
        if variant == "update_note":
            return effects.preflight_update_note(col, body)
        if variant == "restore_note":
            return effects.preflight_restore_note(col, body)
        if variant == "delete_unstudied_created_note":
            return effects.preflight_delete_unstudied_created_note(col, body)
        if variant == "install_model":
            return effects.preflight_install_model(col, body)
        if variant == "store_media":
            data = self._staged(body["sha256"], body["size_bytes"])
            return data, effects.preflight_store_media(col, body["filename"], data,
                                                       body["sha256"])
        if variant == "export_checkpoint":
            target = self._runtime.export_path(None)
            _require(target.parent.is_dir(), "BRIDGE_EXPORT_DIRECTORY_INVALID")
            return None
        raise NativeError("BRIDGE_OPERATION_VARIANT_UNAVAILABLE")

    def _staged(self, sha256, size):
        directory = os.open(self._runtime.staging_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            try:
                handle = os.open(sha256, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=directory)
            except FileNotFoundError:
                raise NativeError("BRIDGE_MEDIA_NOT_STAGED") from None
            try:
                info = os.fstat(handle)
                _require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                         and info.st_size == size, "BRIDGE_MEDIA_STAGE_INVALID")
                data = os.read(handle, size + 1) if size else b""
            finally:
                os.close(handle)
        finally:
            os.close(directory)
        _require(len(data) == size and hashlib.sha256(data).hexdigest() == sha256,
                 "BRIDGE_MEDIA_HASH_MISMATCH")
        return data

    def _perform(self, col, variant, body, prepared):
        if variant == "create_note":
            return effects.perform_create_note(col, body, prepared)
        if variant == "update_note":
            return effects.perform_update_note(col, body, prepared)
        if variant == "restore_note":
            return effects.perform_restore_note(col, body, prepared)
        if variant == "delete_unstudied_created_note":
            return effects.perform_delete_unstudied_created_note(col, body, prepared)
        if variant == "install_model":
            return effects.perform_install_model(col, body, prepared)
        if variant == "store_media":
            data, present = prepared
            return effects.perform_store_media(col, body["filename"], data, body["sha256"],
                                               present)
        raise NativeError("BRIDGE_OPERATION_VARIANT_UNAVAILABLE")

    def _readback(self, col, variant, body, result):
        """Actual post-state evidence; a mismatch raises and leaves `unknown`."""
        if variant in ("create_note", "update_note", "restore_note"):
            observed = effects.observe_note(col, int(result))
            if variant == "create_note":
                _require(body["marker_tag"] in observed["tags"], "BRIDGE_READBACK_MISMATCH")
            _require(observed["fields"] == {**observed["fields"], **body["fields"]},
                     "BRIDGE_READBACK_MISMATCH")
            return {"note_id": observed["id"], "model_id": observed["model_id"],
                    "content_digest": effects.content_digest(observed),
                    "card_ids": [card["id"] for card in observed["cards"]]}
        if variant == "delete_unstudied_created_note":
            _require(effects.observe_or_none(col, body["note_id"]) is None,
                     "BRIDGE_READBACK_MISMATCH")
            return {"note_id": body["note_id"], "removed": True}
        if variant == "install_model":
            model = col.models.get(result)
            _require(model is not None
                     and canonical_manifest.model_digest(model) == body["manifest_digest"],
                     "BRIDGE_READBACK_MISMATCH")
            return {"model_id": model["id"], "manifest_digest": body["manifest_digest"]}
        if variant == "store_media":
            observation = effects.media_observation(col, body["filename"])
            _require(observation is not None and observation["sha256"] == body["sha256"],
                     "BRIDGE_READBACK_MISMATCH")
            return observation
        raise NativeError("BRIDGE_OPERATION_VARIANT_UNAVAILABLE")

    # ------------------------------------------------------------ export
    def _export(self, key, owner, body):
        ledger = self._runtime.ledger
        path = self._runtime.export_path(owner["operation_id"])
        try:
            self._faults.fire("before_export", "export_checkpoint")
            self._runtime.begin_own_export()
        except Exception:  # noqa: BLE001
            ledger.mark_unknown(**owner, reason="native_observation_incomplete")
            self._finish(key)
            return

        def task():
            col = self._runtime.collection()
            col.export_collection_package(str(path), include_media=body["include_media"],
                                          legacy=False)

        def done(_result, error):
            try:
                resumed = self._runtime.end_own_export()
                if error is not None or not resumed:
                    ledger.mark_unknown(**owner, reason="native_observation_incomplete")
                    return
                info = os.lstat(path)
                _require(stat.S_ISREG(info.st_mode), "BRIDGE_EXPORT_UNVERIFIED")
                digest = hashlib.sha256()
                with open(path, "rb") as handle:
                    for chunk in iter(lambda: handle.read(1 << 20), b""):
                        digest.update(chunk)
                ledger.mark_verified(**owner, receipt={
                    "path": str(path), "size_bytes": info.st_size,
                    "sha256": digest.hexdigest()})
            except Exception:  # noqa: BLE001
                try:
                    ledger.mark_unknown(**owner, reason="native_observation_incomplete")
                except Exception:  # noqa: BLE001
                    pass
            finally:
                self._finish(key)

        self._scheduler.background(task, done)


def _wire(value):
    if type(value) is int and 1 <= value <= 9_007_199_254_740_991:
        return value
    raise NativeError("BRIDGE_INSPECT_INVALID")


def _tag(value):
    if (type(value) is not str or not value or len(value.encode("utf-8")) > 100
            or any(char.isspace() or char in '"\\*:()' or ord(char) < 32 for char in value)):
        raise NativeError("BRIDGE_INSPECT_INVALID")
    return value


def _text(value):
    if type(value) is not str or not value or len(value.encode("utf-8")) > 1024:
        raise NativeError("BRIDGE_INSPECT_INVALID")
    return value


def _media(value):
    try:
        return media_name(value)
    except PayloadError:
        raise NativeError("BRIDGE_MEDIA_NAME_INVALID") from None


def _code(error):
    text = str(error)
    return text if text.startswith("BRIDGE_") and len(text) <= 100 else "BRIDGE_PREFLIGHT_FAILED"


def checked_uuid(value):
    try:
        return _uuid(value)
    except ValueError:
        raise NativeError("BRIDGE_OPERATION_ID_INVALID") from None
