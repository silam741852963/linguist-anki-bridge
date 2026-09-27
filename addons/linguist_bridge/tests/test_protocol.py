# SPDX-License-Identifier: GPL-3.0-or-later
import importlib.util
import json
from pathlib import Path
import unittest

PACKAGE = Path(__file__).resolve().parents[1]
ROOT = PACKAGE.parents[1]
spec = importlib.util.spec_from_file_location("bridge_protocol", PACKAGE / "protocol.py")
protocol = importlib.util.module_from_spec(spec)
spec.loader.exec_module(protocol)


class ProtocolTest(unittest.TestCase):
    def arguments(self):
        return dict(bridge_id="c17625b0-7a88-4aab-a8a5-c1d993c72a00",
                    anki_version="fixture", anki_connect_source_digest="a" * 64,
                    api_key_configured=False)

    def test_shared_manifest_fixture_and_no_mutation_or_session_claims(self):
        manifest = protocol.build_capabilities(**self.arguments())
        fixture = json.loads((ROOT / "contracts/v2/fixtures/native-capabilities.json").read_text())
        self.assertEqual(manifest, fixture)
        self.assertEqual(manifest["mutation_variants"], [])
        self.assertIsNone(manifest["collection_session"])
        self.assertEqual(manifest["actions"], ["labCapabilities"])
        manifest["actions"].append("forged")
        self.assertEqual(protocol.build_capabilities(**self.arguments()), fixture)

    def test_invalid_identity_build_digest_and_auth_inputs_fail_closed(self):
        for key, value in [("bridge_id", "00000000-0000-0000-0000-000000000000"),
                           ("bridge_id", "bad"), ("bridge_id", True),
                           ("anki_version", ""), ("anki_version", "private\nvalue"),
                           ("anki_version", "猫" * 43),
                           ("anki_connect_source_digest", "A" * 64),
                           ("anki_connect_source_digest", "secret"),
                           ("api_key_configured", 1)]:
            with self.subTest(key=key, value=value):
                args = self.arguments()
                args[key] = value
                with self.assertRaises(ValueError) as result:
                    protocol.build_capabilities(**args)
                if str(value):
                    self.assertNotIn(str(value), str(result.exception))

    def test_import_does_not_register_or_initialize_services(self):
        package_spec = importlib.util.spec_from_file_location("bridge_scaffold", PACKAGE / "__init__.py")
        package = importlib.util.module_from_spec(package_spec)
        package_spec.loader.exec_module(package)
        self.assertFalse(hasattr(package, "ac"))
        self.assertFalse(hasattr(package, "register"))


if __name__ == "__main__":
    unittest.main()
