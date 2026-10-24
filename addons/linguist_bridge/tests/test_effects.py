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

    def test_split_unit_copies_the_source_schedule_but_no_reviews(self):
        col = FakeNoteCollection(reviews=0)
        source = col.cards[11]
        source.queue, source.type, source.due, source.ivl = 2, 2, 5001, 2899
        source.factor, source.reps, source.lapses, source.flags = 2500, 10, 1, 4
        new = FakeCard(13, 0, 0)
        col.cards = {11: source, 13: new}
        col.card_ids_of_note = lambda _note_id: [13]
        col.updated = []
        col.update_card = col.updated.append
        model = {"tmpls": [{}]}
        entry = {"card_ordinal": 0, "source_card_id": 11,
                 "scheduler": effects._scheduler(source)}
        sources = effects._inherit_sources(col, {"inherit_schedule": [entry]}, model)
        effects._inherit(col, 99, sources)
        self.assertEqual((new.queue, new.type, new.due, new.ivl, new.factor),
                         (2, 2, 5001, 2899, 2500))
        self.assertEqual((new.reps, new.lapses, new.flags, new.reviews), (0, 0, 0, 0))
        self.assertEqual(col.updated, [new])
        # The source was studied since the intent was built: refused before any write.
        stale = dict(entry, scheduler=dict(entry["scheduler"], due="5000"))
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_PRECONDITION_FAILED"):
            effects._inherit_sources(col, {"inherit_schedule": [stale]}, model)
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_INHERIT_ORDINAL_INVALID"):
            effects._inherit_sources(
                col, {"inherit_schedule": [dict(entry, card_ordinal=1)]}, model)
        source.odid = 5
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_FILTERED_DECK"):
            effects._inherit_sources(col, {"inherit_schedule": [entry]}, model)

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


class FakeModelStore:
    """`col.models` for one note type: get returns a copy, update_dict saves."""

    def __init__(self, model):
        self.model = model

    def get(self, mid):
        import copy
        return copy.deepcopy(self.model) if mid == self.model["id"] else None

    def update_dict(self, model):
        self.model = model


class UpdateModelTests(unittest.TestCase):
    def setUp(self):
        from linguist_bridge import manifest
        self.manifest = manifest
        self.model = {"id": 5, "name": "Linguist Vocabulary v3", "css": "old",
                      "flds": [{"name": "Expression", "ord": 0}, {"name": "Meaning", "ord": 1}],
                      "tmpls": [{"name": "Comprehension", "ord": 0, "qfmt": "{{Expression}}",
                                 "afmt": "{{Meaning}}"}]}
        self.target = {"name": "Linguist Vocabulary v3", "version": 3,
                       "fields": ["Expression", "Meaning"], "css": "new",
                       "templates": [{"name": "Comprehension", "ordinal": 0,
                                      "front": '<div data-tags="{{Tags}}">{{Expression}}</div>',
                                      "back": "{{Meaning}}"}]}

    def body(self, **change):
        body = {"manifest": self.target,
                "manifest_digest": self.manifest.managed_digest(self.target),
                "model_id": 5,
                "expected_prior_digest": self.manifest.model_digest(self.model)}
        body.update(change)
        return body

    def test_rewrites_templates_and_css_in_place(self):
        col = FakeCollection(tempfile.mkdtemp())
        col.models = FakeModelStore(self.model)
        body = self.body()
        effects.preflight_update_model(col, body)
        self.assertEqual(effects.perform_update_model(col, body, None), 5)
        self.assertEqual(col.models.model["css"], "new")
        self.assertEqual(self.manifest.model_digest(col.models.model), body["manifest_digest"])

    def test_refuses_a_changed_or_reshaped_note_type(self):
        col = FakeCollection(tempfile.mkdtemp())
        col.models = FakeModelStore(dict(self.model, css="edited by the user"))
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MODEL_CHANGED"):
            effects.preflight_update_model(col, self.body())
        col.models = FakeModelStore(self.model)
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MODEL_ABSENT"):
            effects.preflight_update_model(col, self.body(model_id=6))
        self.target["fields"] = ["Expression", "Meaning", "Extra"]
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MODEL_SHAPE_CHANGED"):
            effects.preflight_update_model(col, self.body())
