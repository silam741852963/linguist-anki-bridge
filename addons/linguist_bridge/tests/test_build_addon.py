# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import zipfile
from uuid import uuid4

SCRIPT = Path(__file__).resolve().parents[2] / "build_read_only_addon.py"
spec = importlib.util.spec_from_file_location("build_read_only_addon", SCRIPT)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class BuildAddonTest(unittest.TestCase):
    def test_archive_is_flat_reproducible_and_contains_only_runtime_and_license(self):
        with tempfile.TemporaryDirectory() as temporary:
            first = builder.build(Path(temporary) / "first.ankiaddon")
            second = builder.build(Path(temporary) / "second.ankiaddon")
            self.assertEqual(hashlib.sha256(first.read_bytes()).digest(),
                             hashlib.sha256(second.read_bytes()).digest())
            with zipfile.ZipFile(first) as archive:
                self.assertEqual(archive.namelist(), sorted([
                    "__init__.py", "LICENSE", "manifest.json", *builder.RUNTIME,
                ]))
                self.assertIn(b"install_anki_hooks()", archive.read("__init__.py"))
                self.assertTrue(archive.read("LICENSE").startswith(b"GNU GENERAL PUBLIC LICENSE"))
                self.assertEqual(json.loads(archive.read("manifest.json"))["package"],
                                 "linguist_bridge")
                self.assertTrue(all("tests/" not in name and "__pycache__" not in name
                                    for name in archive.namelist()))
            with self.assertRaises(FileExistsError):
                builder.build(first)

    def test_relative_or_wrong_extension_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            for path in (Path("relative.ankiaddon"), Path(temporary) / "wrong.zip"):
                with self.assertRaisesRegex(ValueError, "ADDON_OUTPUT_INVALID"):
                    builder.build(path)

    def test_installed_archive_import_only_installs_deferred_hook(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = Path(temporary)
            artifact = builder.build(temporary / "read-only.ankiaddon")
            package = temporary / "installed_addon"
            package.mkdir()
            with zipfile.ZipFile(artifact) as archive:
                archive.extractall(package)
            hook = []
            fake_anki = types.ModuleType("anki")
            fake_anki.__path__ = []
            fake_build = types.ModuleType("anki.buildinfo")
            fake_build.version = "25.09.2"
            fake_build.buildhash = "3d813c83"
            fake_aqt = types.ModuleType("aqt")
            fake_aqt.gui_hooks = types.SimpleNamespace(main_window_did_init=hook)
            fake_aqt.mw = None
            name = "installed_fixture_" + uuid4().hex
            package_spec = importlib.util.spec_from_file_location(
                name, package / "__init__.py", submodule_search_locations=[str(package)])
            module = importlib.util.module_from_spec(package_spec)
            with patch.dict(sys.modules, {"anki": fake_anki, "anki.buildinfo": fake_build,
                                          "aqt": fake_aqt, name: module}):
                package_spec.loader.exec_module(module)
                self.assertEqual(len(hook), 1)
                self.assertFalse(hasattr(module, "STARTUP_ERROR"))
                self.assertEqual(list(package.glob("*.sqlite3")), [])
                for loaded in [key for key in sys.modules if key.startswith(name + ".")]:
                    sys.modules.pop(loaded, None)
