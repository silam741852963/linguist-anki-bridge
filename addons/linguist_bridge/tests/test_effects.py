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


class FakeNote:
    def __init__(self, note_id, mid, fields, tags):
        self.id, self.mid, self.fields, self.tags = note_id, mid, dict(fields), list(tags)

    def keys(self):
        return list(self.fields)

    def __getitem__(self, name):
        return self.fields[name]


class FakeCard:
    def __init__(self, card_id, ordinal, reviews):
        self.id, self.ord, self.did, self.odid = card_id, ordinal, 1, 0
        self.queue = self.type = self.due = self.odue = self.ivl = self.factor = 0
        self.reps, self.lapses, self.left, self.flags = reviews, 0, 0, 0
        self.memory_state = None
        self.reviews = reviews


class FakeModels:
    def get(self, mid):
        return {"id": mid, "name": "Linguist Vocabulary v2"}


class FakeDb:
    def __init__(self, cards):
        self.cards = cards

    def all(self, _query, card_id):
        return [[index, card_id] for index in range(self.cards[card_id].reviews)]


class FakeNoteCollection:
    """Just enough of `Collection` for observe_note and the refusals."""

    def __init__(self, reviews):
        self.note = FakeNote(7, 1001, {"Expression": "食べる"}, ["linguist"])
        self.cards = {11: FakeCard(11, 0, reviews), 12: FakeCard(12, 1, 0)}
        self.models = FakeModels()
        self.db = FakeDb(self.cards)
        self.removed = []

    def get_note(self, note_id):
        if note_id != 7 or self.removed:
            raise KeyError(note_id)
        return self.note

    def card_ids_of_note(self, _note_id):
        return list(self.cards)

    def get_card(self, card_id):
        return self.cards[card_id]

    def remove_notes(self, note_ids):
        self.removed.extend(note_ids)


def digest(_model):
    return "ab" * 32


class EffectTests(unittest.TestCase):
    def test_created_note_is_deleted_only_while_unchanged_and_unstudied(self):
        col = FakeNoteCollection(reviews=1)
        pre = effects.content_digest(effects.observe_note(col, 7, digest))
        body = {"note_id": 7, "expected_pre_digest": pre}
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_STUDIED_NOTE"):
            effects.delete_unstudied_created_note(col, body, digest)
        col = FakeNoteCollection(reviews=0)
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_PRECONDITION_FAILED"):
            effects.delete_unstudied_created_note(
                col, dict(body, expected_pre_digest="stale"), digest)
        self.assertEqual(effects.delete_unstudied_created_note(col, body, digest), 7)
        self.assertEqual(col.removed, [7])
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_NOTE_MISSING"):
            effects.delete_unstudied_created_note(col, body, digest)

    def test_restore_removes_only_listed_unstudied_cards(self):
        col = FakeNoteCollection(reviews=0)
        pre = effects.content_digest(effects.observe_note(col, 7, digest))
        reverse = {"source_model_id": 1001, "target_model_id": 1,
                   "target_model_name": "Basic", "ordinal_map": [{"source": 0, "target": 0}]}
        body = {"note_id": 7, "expected_pre_digest": pre, "migration": reverse,
                "fields": {}, "tags": [], "card_decks": [{"card_id": 11, "deck_id": 1}],
                "removed_card_ids": []}
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_REVERSE_MAPPING_MISMATCH"):
            effects.restore_note(col, body, digest)
        col.cards[12].reviews = 1
        pre = effects.content_digest(effects.observe_note(col, 7, digest))
        studied = dict(body, expected_pre_digest=pre, removed_card_ids=[12])
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_STUDIED_CARD_REMOVAL"):
            effects.restore_note(col, studied, digest)
        without = dict(body, expected_pre_digest=pre, migration=None, removed_card_ids=[12])
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_REMOVAL_REQUIRES_MAPPING"):
            effects.restore_note(col, without, digest)

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
