# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import importlib.util
from pathlib import Path
import stat
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.compatibility import READ_COMPATIBILITY
from linguist_bridge.registration import RegistrationError
from linguist_bridge.startup import StartupError, activate_read_only
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
        state = Path(self.window.pm.base) / "linguist-anki-bridge-native"
        with patch.dict(READ_COMPATIBILITY, {("test-anki", "test-build"): self.pins}):
            callback = startup.install_read_only_hook(
                hook, lambda: self.window, "test-anki", "test-build")
            self.assertEqual(hook, [callback])
            self.assertFalse(state.exists())
            self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))
            callback()
            self.assertIsNone(startup.last_startup_error)
            self.assertTrue(state.exists())
            first = self.module.ac.labCapabilities()["bridge_id"]
            callback()
            self.assertEqual(self.module.ac.labCapabilities()["bridge_id"], first)
            startup.shutdown_read_only()
        self.assertFalse(hasattr(self.module.AnkiConnect, "labCapabilities"))

    def test_unsupported_hook_build_does_not_append_or_create_state(self):
        hook = []
        with self.assertRaisesRegex(RegistrationError, "BRIDGE_ANKI_BUILD_UNSUPPORTED"):
            startup.install_read_only_hook(hook, lambda: self.window, "unknown", "unknown")
        self.assertEqual(hook, [])
        self.assertFalse((Path(self.window.pm.base) / "linguist-anki-bridge-native").exists())
