# SPDX-License-Identifier: GPL-3.0-or-later
"""lab-native-v1 actions over a real Anki `Collection` in a temporary directory.

Runs with a Python that can import Anki's library (on this machine
`/usr/bin/python3.14`); skipped otherwise. AnkiConnect is a small fixture
module with the same dispatcher contract; the full desktop path is exercised
by the disposable desktop scenarios.
"""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge import compatibility, effects, manifest  # noqa: E402
from linguist_bridge.payloads import VOCAB_FIELDS  # noqa: E402
from linguist_bridge.startup import activate_native  # noqa: E402

try:
    from anki.collection import Collection
except ImportError:  # pragma: no cover - depends on the interpreter
    Collection = None

UTIL = '''def api(*versions):
    def decorate(function):
        function.api = True
        function.versions = versions
        return function
    return decorate
KEY = ["test-key"]
def setting(key):
    return KEY[0] if key == "apiKey" else None
'''
MAIN = '''class AnkiConnect:
    def handler(self, request):
        if request.get("key") != util.setting("apiKey"):
            raise Exception("valid api key must be provided")
        return getattr(self, request['action'])(**request.get('params', {}))
    @util.api()
    def apiReflect(self, scopes):
        return {'actions': [name for name in dir(type(self))
                if getattr(getattr(type(self), name), 'api', False)]}
ac = AnkiConnect()
'''
PLAN = "lab-jcs-v1:plan:" + "a" * 64
MODEL = "lab-jcs-v1:model-install:" + "b" * 64
CHECKPOINT = "lab-jcs-v1:checkpoint:" + "c" * 64


class Hook(list):
    def __call__(self, *args):
        for callback in list(self):
            callback(*args)


class Hooks:
    def __init__(self):
        for name in ("collection_did_load", "collection_will_temporarily_close",
                     "collection_did_temporarily_close", "profile_will_close"):
            setattr(self, name, Hook())


class Scheduler:
    """Deferred critical sections, run explicitly like the main loop would."""

    def __init__(self):
        self.pending = []

    def critical(self, function):
        self.pending.append(function)

    def background(self, task, on_done):
        try:
            result, error = task(), None
        except Exception as failure:  # noqa: BLE001
            result, error = None, failure
        on_done(result, error)

    def run(self):
        while self.pending:
            self.pending.pop(0)()


class Window:
    def __init__(self, base, col, hooks):
        self.col = col
        self.hooks = hooks
        path = col.path
        self.pm = types.SimpleNamespace(base=str(base), name="Disposable",
                                        collectionPath=lambda: path)

    def reopen(self):
        self.col.reopen()
        self.hooks.collection_did_temporarily_close(self.col)


def managed_manifest():
    fields = sorted(VOCAB_FIELDS)
    return {"name": "Linguist Vocabulary v2", "version": 2, "fields": fields,
            "templates": [{"name": "Recognition", "ordinal": 0, "front": "{{Expression}}",
                           "back": "{{Meaning}}"},
                          {"name": "Production", "ordinal": 1,
                           "front": "{{#EnableProduction}}{{Meaning}}{{/EnableProduction}}",
                           "back": "{{Expression}}"}],
            "css": ".card { color: black; }"}


@unittest.skipIf(Collection is None, "Anki's Python library is not importable")
class NativeRuntimeTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        self.addon = root / "ankiconnect"
        self.addon.mkdir()
        (self.addon / "util.py").write_text(UTIL)
        (self.addon / "__init__.py").write_text(MAIN)
        pins = {name: hashlib.sha256((self.addon / name).read_bytes()).hexdigest()
                for name in ("__init__.py", "util.py")}
        self.name = "native_fixture_" + uuid4().hex
        spec = importlib.util.spec_from_file_location(self.name + ".util", self.addon / "util.py")
        self.util = importlib.util.module_from_spec(spec)
        sys.modules[self.name + ".util"] = self.util
        spec.loader.exec_module(self.util)
        spec = importlib.util.spec_from_file_location(self.name, self.addon / "__init__.py")
        self.module = importlib.util.module_from_spec(spec)
        self.module.util = self.util
        sys.modules[self.name] = self.module
        spec.loader.exec_module(self.module)
        base = root / "Anki2"
        (base / "Disposable").mkdir(parents=True)
        self.col = Collection(str(base / "Disposable" / "collection.anki2"))
        self.hooks = Hooks()
        self.window = Window(base, self.col, self.hooks)
        self.scheduler = Scheduler()
        self.fault_file = root / "fault.json"
        from linguist_bridge.native import FaultInjector
        with patch.dict(compatibility.READ_COMPATIBILITY, {("test", "build"): pins}), \
                patch.object(compatibility, "WRITE_COMPATIBILITY", {("test", "build")}), \
                patch("linguist_bridge.startup.write_supported", lambda *_: True):
            self.runtime = activate_native(
                main_window=self.window, anki_connect_module=self.module,
                anki_version="test", build_hash="build", gui_hooks=self.hooks,
                scheduler=self.scheduler, faults=FaultInjector(self.fault_file))
        self.runtime.bind_session_hooks(self.hooks)
        self.runtime._on_opened(self.col)

    def tearDown(self):
        self.runtime.close()
        self.col.close()
        sys.modules.pop(self.name, None)
        sys.modules.pop(self.name + ".util", None)
        self.temp.cleanup()

    def call(self, action, **params):
        return self.module.ac.handler({"action": action, "params": params, "key": "test-key"})

    def session(self):
        return self.call("labCapabilities")["collection_session"]

    def begin(self, approved=PLAN):
        session = self.session()
        binding = dict(session, bridge_id=self.runtime.bridge_id)
        owner = self.call("labBegin", binding=binding, approved_digest=approved)
        owner["approved"] = approved
        return session, owner

    def mutate(self, session, owner, variant, body, operation_id=None):
        operation_id = operation_id or str(uuid4())
        if variant == "create_note":
            body = dict(body, marker_tag="lab_op_" + operation_id.replace("-", ""))
            body["tags"] = sorted({*body["tags"], body["marker_tag"]})
        payload = json.dumps({"schema_version": 1, "variant": variant, "body": body},
                             sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        status = self.call("labMutate", lineage_id=session["lineage_id"],
                           operation_id=operation_id, session_epoch=session["session_epoch"],
                           owner_token=owner["owner_token"], fence=owner["fence"],
                           approved_digest=owner["approved"], variant=variant, payload=payload)
        return operation_id, payload, status

    def status(self, session, operation_id):
        return self.call("labOperationStatus", lineage_id=session["lineage_id"],
                         operation_id=operation_id)

    def install(self):
        session, owner = self.begin(MODEL)
        model = managed_manifest()
        operation, _, queued = self.mutate(session, owner, "install_model", {
            "manifest": model, "manifest_digest": manifest.managed_digest(model),
            "expected_absent": True})
        self.assertEqual(queued["state"], "queued")
        self.scheduler.run()
        installed = self.status(session, operation)
        self.call("labEnd", owner_token=owner["owner_token"], fence=owner["fence"])
        return installed

    def create_body(self, deck_id):
        model = managed_manifest()
        fields = {name: "" for name in VOCAB_FIELDS}
        fields.update(Expression="食べる", Meaning="to eat", Language="ja")
        return {"model_name": model["name"], "model_manifest_digest": manifest.managed_digest(model),
                "deck_id": str(deck_id), "fields": fields, "tags": ["linguist"],
                "marker_tag": "", "source_plan_digest": PLAN, "checkpoint_digest": "c" * 64,
                "binding": {"profile_fingerprint": "d" * 64, "path_fingerprint": "e" * 64},
                "expected_absent": True}

    def test_capabilities_declare_all_variants_only_with_key_and_session(self):
        manifest_ = self.call("labCapabilities")
        self.assertEqual(sorted(manifest_["actions"]), sorted([
            "labCapabilities", "labBegin", "labInspect", "labMutate", "labOperationStatus",
            "labRebind", "labEnd"]))
        self.assertEqual(len(manifest_["mutation_variants"]), 7)
        self.assertTrue(manifest_["api_key_configured"])
        self.util.KEY[0] = None
        unauthenticated = self.module.ac.handler({"action": "labCapabilities"})
        self.assertEqual(unauthenticated["mutation_variants"], [])
        with self.assertRaisesRegex(RuntimeError, "BRIDGE_AUTH_REQUIRED"):
            self.module.ac.handler({"action": "labBegin", "params": {
                "binding": {}, "approved_digest": PLAN}})

    def test_install_create_update_media_and_delete_through_the_ledger(self):
        installed = self.install()
        session, owner = self.begin()
        self.assertEqual(installed["state"], "verified", installed)
        model_id = installed["receipt"]["model_id"]
        self.assertEqual(manifest.model_digest(self.col.models.get(model_id)),
                         installed["receipt"]["manifest_digest"])
        deck_id = self.col.decks.id("Japanese::Vocab")
        operation, _, queued = self.mutate(session, owner, "create_note", self.create_body(deck_id))
        self.assertEqual(queued["state"], "queued")
        # Not dispatched until the serialized critical section runs.
        self.assertEqual(self.col.find_notes("食べる"), [])
        self.scheduler.run()
        created = self.status(session, operation)
        self.assertEqual(created["state"], "verified", created)
        note_id = created["receipt"]["note_id"]
        tagged = self.call("labInspect", kind="notes_tagged",
                           tag="lab_op_" + operation.replace("-", ""),
                           session_epoch=session["session_epoch"])
        self.assertEqual([note["id"] for note in tagged], [note_id])
        # Duplicate UUID: same state and receipt, no second note.
        _, _, again = self.mutate(session, owner, "create_note", self.create_body(deck_id),
                                  operation_id=operation)
        self.assertEqual(again, created)
        self.scheduler.run()
        self.assertEqual(len(self.col.find_notes("食べる")), 1)
        # Media is staged by reference, verified and never overwritten.
        data = b"\x00audio"
        digest = hashlib.sha256(data).hexdigest()
        (self.runtime.staging_dir / digest).write_bytes(data)
        operation, _, _ = self.mutate(session, owner, "store_media", {
            "filename": "eat.ogg", "sha256": digest, "size_bytes": len(data),
            "staged_asset": digest})
        self.scheduler.run()
        stored = self.status(session, operation)
        self.assertEqual(stored["receipt"], {"filename": "eat.ogg", "sha256": digest,
                                             "size_bytes": len(data)})
        media = self.call("labInspect", kind="media_bytes", filename="eat.ogg", max_bytes=1024,
                          session_epoch=session["session_epoch"])
        self.assertEqual(base64.b64decode(media), data)
        # Update under the content precondition; card IDs and history are kept.
        card_id = self.col.card_ids_of_note(note_id)[0]
        card = self.col.get_card(card_id)
        card.start_timer()
        self.col.sched.answerCard(card, 3)
        observed = self.call("labInspect", kind="note", note_id=note_id,
                             session_epoch=session["session_epoch"])
        fields = dict(observed["fields"], Meaning="to eat (food)")
        operation, _, _ = self.mutate(session, owner, "update_note", {
            "note_id": note_id, "expected_pre_digest": effects.content_digest(observed),
            "migration": None, "fields": fields, "add_tags": ["revamped"], "deck_id": deck_id})
        self.scheduler.run()
        updated = self.status(session, operation)
        self.assertEqual(updated["state"], "verified", updated)
        after = self.call("labInspect", kind="note", note_id=note_id,
                          session_epoch=session["session_epoch"])
        self.assertEqual(after["fields"]["Meaning"], "to eat (food)")
        self.assertEqual([c["id"] for c in after["cards"]], [c["id"] for c in observed["cards"]])
        self.assertEqual(after["cards"][0]["history_digest"], observed["cards"][0]["history_digest"])
        # A stale precondition is refused before any write.
        operation, _, _ = self.mutate(session, owner, "update_note", {
            "note_id": note_id, "expected_pre_digest": effects.content_digest(observed),
            "migration": None, "fields": fields, "add_tags": [], "deck_id": deck_id})
        self.scheduler.run()
        refused = self.status(session, operation)
        self.assertEqual((refused["state"], refused["reason"], refused["receipt"]),
                         ("failed_before_write", "preflight_rejected",
                          {"code": "BRIDGE_PRECONDITION_FAILED"}))
        # A studied created note is never deleted.
        operation, _, _ = self.mutate(session, owner, "delete_unstudied_created_note", {
            "note_id": note_id, "expected_pre_digest": effects.content_digest(after)})
        self.scheduler.run()
        self.assertEqual(self.status(session, operation)["receipt"],
                         {"code": "BRIDGE_STUDIED_NOTE"})
        self.assertTrue(self.col.find_notes(f"nid:{note_id}"))
        self.assertTrue(self.call("labEnd", owner_token=owner["owner_token"], fence=owner["fence"]))

    def test_stale_fence_session_change_and_in_flight_owner(self):
        self.install()
        session, owner = self.begin()
        deck_id = self.col.decks.id("Japanese::Vocab")
        operation, _, _ = self.mutate(session, owner, "create_note", self.create_body(deck_id))
        # In-flight work blocks a new owner; it cannot be fenced out mid-run.
        with self.assertRaisesRegex(RuntimeError, "BRIDGE_OPERATION_PENDING"):
            self.begin()
        self.scheduler.run()
        _, second = self.begin()
        self.assertEqual(second["fence"], owner["fence"] + 1)
        with self.assertRaisesRegex(RuntimeError, "BRIDGE_OWNER_STALE"):
            self.mutate(session, owner, "create_note", self.create_body(deck_id))
        # A collection load produces a new epoch: the old one cannot dispatch.
        self.hooks.collection_did_load(self.col)
        current = self.session()
        self.assertNotEqual(current["session_epoch"], session["session_epoch"])
        self.assertEqual(current["lineage_id"], session["lineage_id"])
        with self.assertRaisesRegex(RuntimeError, "BRIDGE_SESSION_CONFLICT"):
            self.mutate(session, second, "create_note", self.create_body(deck_id))
        rebind = self.call("labRebind", lineage_id=session["lineage_id"],
                           previous_epoch=session["session_epoch"])
        self.assertFalse(rebind["previous_epoch_current"])
        self.assertEqual(rebind["collection_session"], current)
        # Session changes between acceptance and the critical section.
        session, owner = self.begin()
        operation, _, _ = self.mutate(session, owner, "create_note", self.create_body(deck_id))
        self.hooks.collection_did_load(self.col)
        self.scheduler.run()
        failed = self.status(session, operation)
        self.assertEqual((failed["state"], failed["reason"]),
                         ("failed_before_write", "session_changed"))

    def test_crash_after_effect_is_classified_unknown_by_the_next_owner(self):
        self.install()
        session, owner = self.begin()
        deck_id = self.col.decks.id("Japanese::Vocab")
        self.fault_file.write_text(json.dumps({"point": "after_effect", "action": "disk_full"}))
        operation, _, _ = self.mutate(session, owner, "create_note", self.create_body(deck_id))
        self.scheduler.run()
        status = self.status(session, operation)
        # The effect happened; the failure after it is never reported as success.
        self.assertEqual((status["state"], status["reason"]),
                         ("unknown", "native_observation_incomplete"))
        self.assertEqual(len(self.col.find_notes("食べる")), 1)
        # Disk full at the running boundary leaves the row queued: no effect.
        self.fault_file.write_text(json.dumps({"point": "before_running", "action": "disk_full"}))
        operation, _, _ = self.mutate(session, owner, "create_note", self.create_body(deck_id))
        self.scheduler.run()
        self.assertEqual(self.status(session, operation)["state"], "queued")
        self.assertEqual(len(self.col.find_notes("食べる")), 1)
        _, fresh = self.begin()
        status = self.status(session, operation)
        self.assertEqual((status["state"], status["reason"]),
                         ("failed_before_write", "worker_lost_before_write"))
        self.assertEqual(fresh["fence"], owner["fence"] + 1)

    def test_export_keeps_the_epoch_and_reports_the_package(self):
        self.install()
        session, owner = self.begin(CHECKPOINT)
        operation, _, _ = self.mutate(session, owner, "export_checkpoint",
                                      {"include_media": True, "include_scheduling": True})
        self.scheduler.run()
        status = self.status(session, operation)
        self.assertEqual(status["state"], "verified", status)
        receipt = status["receipt"]
        data = Path(receipt["path"]).read_bytes()
        self.assertEqual((len(data), hashlib.sha256(data).hexdigest()),
                         (receipt["size_bytes"], receipt["sha256"]))
        self.assertTrue(receipt["path"].startswith(str(self.runtime.exports_dir)))
        self.assertEqual(self.session(), session)
        scope = self.call("labInspect", kind="scope", note_ids=[],
                          requirement={"scheduling": True, "media": True, "schema": True},
                          session_epoch=session["session_epoch"])
        self.assertEqual(scope["schema_version"], 1)


if __name__ == "__main__":
    unittest.main()
