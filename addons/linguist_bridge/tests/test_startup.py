# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import importlib.util
from pathlib import Path
import os
import stat
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.compatibility import READ_COMPATIBILITY
from linguist_bridge.inspection import InspectionError
from linguist_bridge.registration import RegistrationError
from linguist_bridge.startup import StartupError, activate_read_only
from linguist_bridge.session import SessionError
from linguist_bridge import startup


UTIL = '''def api(*versions):
    def decorate(function):
        function.api = True
        function.versions = versions
        return function
    return decorate
def setting(key):
    return "test-key" if key == "apiKey" else None
'''
MAIN = '''class AnkiConnect:
    def handler(self, request):
        return getattr(self, request['action'])(**request.get('params', {}))
    @util.api()
    def standard(self):
        return 'unchanged'
    @util.api()
    def apiReflect(self, scopes):
        return {'actions': [name for name in dir(type(self))
                if getattr(getattr(type(self), name), 'api', False)]}
ac = AnkiConnect()
'''


class StartupTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.addon = self.root / "ankiconnect"
        self.addon.mkdir()
        (self.addon / "util.py").write_text(UTIL)
        (self.addon / "__init__.py").write_text(MAIN)
        self.pins = {name: hashlib.sha256((self.addon / name).read_bytes()).hexdigest()
                     for name in ("__init__.py", "util.py")}
        self.name = "startup_fixture_" + uuid4().hex
        util_spec = importlib.util.spec_from_file_location(self.name + ".util", self.addon / "util.py")
        self.util = importlib.util.module_from_spec(util_spec)
        sys.modules[self.name + ".util"] = self.util
        util_spec.loader.exec_module(self.util)
        main_spec = importlib.util.spec_from_file_location(self.name, self.addon / "__init__.py")
        self.module = importlib.util.module_from_spec(main_spec)
        self.module.util = self.util
        sys.modules[self.name] = self.module
        main_spec.loader.exec_module(self.module)
        base = self.root / "Anki2"
        base.mkdir()
        self.window = types.SimpleNamespace(pm=types.SimpleNamespace(base=str(base)))

    def tearDown(self):
        sys.modules.pop(self.name, None)
        sys.modules.pop(self.name + ".util", None)
        self.temp.cleanup()

    def activate(self):
        with patch.dict(READ_COMPATIBILITY, {("test-anki", "test-build"): self.pins}):
            return activate_read_only(main_window=self.window,
                                      anki_connect_module=self.module,
                                      anki_version="test-anki", build_hash="test-build")

    def test_activation_registers_only_reads_and_reuses_private_identity(self):
        runtime = self.activate()
        state = Path(self.window.pm.base) / "linguist-anki-bridge-native"
        self.assertEqual(stat.S_IMODE(state.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE((state / "native-sidecar.sqlite3").stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE((state / "native-operations.sqlite3").stat().st_mode), 0o600)
        manifest = self.module.ac.handler({"action": "labCapabilities"})
        self.assertEqual(manifest["actions"], ["labCapabilities", "labOperationStatus"])
        self.assertEqual(manifest["mutation_variants"], [])
        self.assertIsNone(manifest["collection_session"])
        self.assertTrue(manifest["api_key_configured"])
        self.assertEqual(self.module.ac.standard(), "unchanged")
        self.assertFalse(hasattr(self.module.AnkiConnect, "labMutate"))
        runtime.close()
        self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))
        self.assertFalse(hasattr(self.module.AnkiConnect, "labOperationStatus"))
        reopened = self.activate()
        self.assertEqual(self.module.ac.labCapabilities()["bridge_id"], manifest["bridge_id"])
        reopened.close()

    def test_corrupt_lineage_sidecar_prevents_registration(self):
        runtime = self.activate()
        runtime.close()
        state = Path(self.window.pm.base) / "linguist-anki-bridge-native"
        (state / "native-sidecar.sqlite3").write_bytes(b"invalid sqlite")
        with self.assertRaisesRegex(Exception, "BRIDGE_SIDECAR"):
            self.activate()
        self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))

    def test_collection_hooks_rotate_epochs_and_invalidate_replaced_files(self):
        collection = Path(self.window.pm.base) / "collection.anki2"
        collection.write_bytes(b"collection")
        self.window.pm.name = "Test"
        self.window.pm.collectionPath = lambda: str(collection)
        self.window.col = object()
        hooks = types.SimpleNamespace(**{
            name: [] for name in (
                "collection_did_load", "collection_will_temporarily_close",
                "collection_did_temporarily_close", "profile_will_close")})
        runtime = self.activate()
        runtime.bind_session_hooks(hooks)
        with self.assertRaisesRegex(SessionError, "BRIDGE_SESSION_UNAVAILABLE"):
            runtime.observed_session()
        hooks.collection_did_load[0](self.window.col)
        first = runtime.observed_session()
        self.assertEqual(self.module.ac.labCapabilities()["collection_session"], first)
        self.assertEqual(runtime.last_session_error, None)
        hooks.collection_will_temporarily_close[0](self.window.col)
        with self.assertRaisesRegex(SessionError, "BRIDGE_SESSION_UNAVAILABLE"):
            runtime.observed_session()
        self.assertIsNone(self.module.ac.labCapabilities()["collection_session"])
        hooks.collection_did_temporarily_close[0](self.window.col)
        second = runtime.observed_session()
        self.assertEqual(first["lineage_id"], second["lineage_id"])
        self.assertNotEqual(first["session_epoch"], second["session_epoch"])
        replacement = collection.with_suffix(".replacement")
        replacement.write_bytes(b"replacement")
        os.replace(replacement, collection)
        with self.assertRaisesRegex(SessionError, "BRIDGE_SESSION_CONFLICT"):
            runtime.observed_session()
        self.assertIsNone(self.module.ac.labCapabilities()["collection_session"])
        with self.assertRaisesRegex(SessionError, "BRIDGE_SESSION_UNAVAILABLE"):
            runtime.observed_session()
        hooks.collection_did_load[0](self.window.col)
        self.assertNotEqual(second["session_epoch"],
                            runtime.observed_session()["session_epoch"])
        hooks.profile_will_close[0]()
        self.assertIsNone(self.module.ac.labCapabilities()["collection_session"])
        with self.assertRaisesRegex(SessionError, "BRIDGE_SESSION_UNAVAILABLE"):
            runtime.observed_session()
        runtime.close()
        self.assertTrue(all(not callbacks for callbacks in vars(hooks).values()))

    def test_invalid_collection_load_keeps_read_actions_without_session(self):
        self.window.pm.name = "Test"
        self.window.pm.collectionPath = lambda: str(Path(self.window.pm.base) / "missing.anki2")
        self.window.col = object()
        hooks = types.SimpleNamespace(**{
            name: [] for name in (
                "collection_did_load", "collection_will_temporarily_close",
                "collection_did_temporarily_close", "profile_will_close")})
        runtime = self.activate()
        runtime.bind_session_hooks(hooks)
        hooks.collection_did_load[0](self.window.col)
        self.assertIsNotNone(runtime.last_session_error)
        self.assertIsNone(self.module.ac.labCapabilities()["collection_session"])
        with self.assertRaisesRegex(SessionError, "BRIDGE_SESSION_UNAVAILABLE"):
            runtime.observed_session()
        runtime.close()

    def test_internal_inspection_requires_stable_observed_epoch(self):
        collection = Path(self.window.pm.base) / "collection.anki2"
        collection.write_bytes(b"collection")
        self.window.pm.name = "Test"
        self.window.pm.collectionPath = lambda: str(collection)
        self.window.col = object()
        hooks = types.SimpleNamespace(**{
            name: [] for name in (
                "collection_did_load", "collection_will_temporarily_close",
                "collection_did_temporarily_close", "profile_will_close")})
        runtime = self.activate()
        try:
            runtime.bind_session_hooks(hooks)
            hooks.collection_did_load[0](self.window.col)
            epoch = runtime.observed_session()["session_epoch"]
            with patch("linguist_bridge.startup.inspect_note_twice",
                       return_value={"note_id": "123", "atomic_snapshot_verified": False}) as inspect:
                report = runtime.inspect_note("123", epoch)
                inspect.assert_called_once_with(self.window.col, "123")
            self.assertEqual(report["collection_session"]["session_epoch"], epoch)
            self.assertFalse(report["atomic_snapshot_verified"])
            for stale in ("not-a-uuid", str(uuid4())):
                with patch("linguist_bridge.startup.inspect_note_twice") as inspect:
                    with self.assertRaisesRegex(InspectionError,
                                                "BRIDGE_INSPECT_SESSION_CONFLICT"):
                        runtime.inspect_note("123", stale)
                    inspect.assert_not_called()
            def change_profile(_collection, _note_id):
                self.window.pm.name = "Other"
                return {"note_id": "123"}
            with patch("linguist_bridge.startup.inspect_note_twice",
                       side_effect=change_profile):
                with self.assertRaisesRegex(InspectionError,
                                            "BRIDGE_INSPECT_SESSION_CONFLICT"):
                    runtime.inspect_note("123", epoch)
            self.assertIsNone(self.module.ac.labCapabilities()["collection_session"])
            self.window.pm.name = "Test"
            hooks.collection_did_load[0](self.window.col)
            epoch = runtime.observed_session()["session_epoch"]
            def replace_file(_collection, _note_id):
                replacement = collection.with_suffix(".replacement")
                replacement.write_bytes(b"replacement")
                os.replace(replacement, collection)
                return {"note_id": "123"}
            with patch("linguist_bridge.startup.inspect_note_twice",
                       side_effect=replace_file):
                with self.assertRaisesRegex(InspectionError,
                                            "BRIDGE_INSPECT_SESSION_CONFLICT"):
                    runtime.inspect_note("123", epoch)
            self.assertIsNone(self.module.ac.labCapabilities()["collection_session"])
            self.assertFalse(hasattr(self.module.AnkiConnect, "labInspect"))
        finally:
            runtime.close()

    def test_unknown_build_and_changed_source_create_no_state(self):
        state = Path(self.window.pm.base) / "linguist-anki-bridge-native"
        with self.assertRaisesRegex(RegistrationError, "BRIDGE_ANKI_BUILD_UNSUPPORTED"):
            activate_read_only(main_window=self.window, anki_connect_module=self.module,
                               anki_version="unknown", build_hash="unknown")
        self.assertFalse(state.exists())
        (self.addon / "util.py").write_text(UTIL + "\n# changed\n")
        with self.assertRaisesRegex(StartupError, "BRIDGE_SOURCE_PIN_MISMATCH"):
            self.activate()
        self.assertFalse(state.exists())

    def test_unusable_state_parent_rejects_activation(self):
        self.window.pm.base = str(self.root / "missing")
        with self.assertRaisesRegex(StartupError, "BRIDGE_STATE_PARENT_INVALID"):
            self.activate()
        self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))

    def test_hook_defers_state_and_registration_until_main_window_ready(self):
        hook = []
        session_hooks = types.SimpleNamespace(**{
            name: [] for name in (
                "collection_did_load", "collection_will_temporarily_close",
                "collection_did_temporarily_close", "profile_will_close")})
        state = Path(self.window.pm.base) / "linguist-anki-bridge-native"
        with patch.dict(READ_COMPATIBILITY, {("test-anki", "test-build"): self.pins}):
            callback = startup.install_read_only_hook(
                hook, lambda: self.window, "test-anki", "test-build",
                session_hooks=session_hooks)
            self.assertEqual(hook, [callback])
            self.assertFalse(state.exists())
            self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))
            callback()
            self.assertIsNone(startup.last_startup_error)
            self.assertTrue(state.exists())
            self.assertTrue(all(len(callbacks) == 1
                                for callbacks in vars(session_hooks).values()))
            first = self.module.ac.labCapabilities()["bridge_id"]
            callback()
            self.assertEqual(self.module.ac.labCapabilities()["bridge_id"], first)
            startup.shutdown_read_only()
            self.assertTrue(all(not callbacks for callbacks in vars(session_hooks).values()))
        self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))

    def test_unsupported_hook_build_does_not_append_or_create_state(self):
        hook = []
        with self.assertRaisesRegex(RegistrationError, "BRIDGE_ANKI_BUILD_UNSUPPORTED"):
            startup.install_read_only_hook(hook, lambda: self.window, "unknown", "unknown")
        self.assertEqual(hook, [])
        self.assertFalse((Path(self.window.pm.base) / "linguist-anki-bridge-native").exists())

    def test_missing_session_hook_rolls_back_read_actions(self):
        hook = []
        missing = types.SimpleNamespace(collection_did_load=[])
        with patch.dict(READ_COMPATIBILITY, {("test-anki", "test-build"): self.pins}):
            callback = startup.install_read_only_hook(
                hook, lambda: self.window, "test-anki", "test-build",
                session_hooks=missing)
            callback()
            self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))
            self.assertEqual(missing.collection_did_load, [])
            self.assertIsNone(startup._runtime)
