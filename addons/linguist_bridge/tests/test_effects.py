# SPDX-License-Identifier: GPL-3.0-or-later
"""Anki-free checks of the unregistered native effect helpers."""
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge import effects  # noqa: E402


class FakeMedia:
    def __init__(self, directory):
        self.directory = directory

    def dir(self):
        return self.directory

    def write_data(self, name, data):
        with open(os.path.join(self.directory, name), "xb") as file:
            file.write(data)
        return name


class FakeCollection:
    def __init__(self, directory):
        self.media = FakeMedia(directory)


class EffectTests(unittest.TestCase):
    def test_precondition_digest_matches_rust_vector(self):
        observed = {
            "model_name": "Linguist Vocabulary v2",
            "model_manifest_digest": "ab" * 32,
            "fields": {"Expression": "食べる", "Meaning": "to \"eat\"\n"},
            "tags": ["zeta", "alpha", "zeta"],
            "cards": [
                {"id": 1700000000004, "ordinal": 1, "deck_id": 1, "original_deck_id": 0},
                {"id": 1700000000003, "ordinal": 0, "deck_id": 1, "original_deck_id": 0},
            ],
        }
        self.assertEqual(effects.content_digest(observed), "lab-jcs-v1:lab-apply-precondition-v1:719d9ff2eac8a52ebe85cc0ed0c38c91a46ceafb852850fc65536615488c314c")

    def test_media_is_stored_once_and_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            col = FakeCollection(directory)
            data = b"approved"
            digest = hashlib.sha256(data).hexdigest()
            self.assertEqual(effects.store_media(col, "a.ogg", data, digest), "a.ogg")
            self.assertEqual(effects.store_media(col, "a.ogg", data, digest), "a.ogg")
            other = b"other"
            with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MEDIA_COLLISION"):
                effects.store_media(col, "a.ogg", other, hashlib.sha256(other).hexdigest())
            with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MEDIA_HASH_MISMATCH"):
                effects.store_media(col, "b.ogg", data, "0" * 64)
            for name in ("../x", ".hidden", "a/b", "", "c:d"):
                with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MEDIA_NAME_INVALID"):
                    effects.store_media(col, name, data, digest)
            self.assertEqual(effects.media_sha256(col, "a.ogg"), digest)


if __name__ == "__main__":
    unittest.main()
