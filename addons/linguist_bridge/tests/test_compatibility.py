# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.compatibility import read_source_pins, register_supported_read_actions
from linguist_bridge.registration import RegistrationError


class CompatibilityTest(unittest.TestCase):
    def test_exact_build_returns_isolated_source_pins(self):
        pins = read_source_pins("25.09.2", "3d813c83")
        self.assertEqual(set(pins), {"__init__.py", "util.py"})
        self.assertTrue(all(len(digest) == 64 for digest in pins.values()))
        pins["__init__.py"] = "forged"
        self.assertNotEqual(read_source_pins("25.09.2", "3d813c83")["__init__.py"], "forged")

    def test_unknown_or_untyped_build_fails_closed(self):
        for version, build in [
            ("25.09.2", "changed"), ("25.09.3", "3d813c83"),
            ("", "3d813c83"), (None, "3d813c83"),
            ("25.09.2", True),
        ]:
            with self.subTest(version=version, build=build):
                with self.assertRaisesRegex(RegistrationError, "BRIDGE_ANKI_BUILD_UNSUPPORTED"):
                    read_source_pins(version, build)

    def test_unsupported_build_never_reaches_registration(self):
        class FakeModule:
            pass
        with self.assertRaisesRegex(RegistrationError, "BRIDGE_ANKI_BUILD_UNSUPPORTED"):
            register_supported_read_actions(FakeModule(), "unknown", "unknown", lambda: {})
        self.assertEqual(vars(FakeModule).get("labCapabilities"), None)
