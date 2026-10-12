# SPDX-License-Identifier: GPL-3.0-or-later
"""WP-24 duplicate merge effects on a real (temporary) Anki collection."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge import effects, inspection, payloads  # noqa: E402

try:
    from anki.collection import Collection
except ImportError:  # pragma: no cover - plain Python without Anki
    Collection = None


def recreate_body(evidence, model_id):
    note = evidence["note"]
    cards = [{key: card[key] for key in (
        "id", "ordinal", "deck_id", "original_deck_id", "type", "queue", "due", "interval",
        "ease_factor", "repetitions", "lapses", "remaining_steps", "original_due", "flags",
        "custom_data")} for card in evidence["cards"]]
    for card in cards:
        card["id"], card["deck_id"] = int(card["id"]), int(card["deck_id"])
        card["original_deck_id"] = int(card["original_deck_id"])
    reviews = [dict(review) for card in evidence["cards"] for review in card["reviews"]]
    return {"note": {"id": int(evidence["note_id"]), "guid": note["guid"],
                     "model_id": model_id, "fields": list(note["fields"].values()),
                     "tags": note["tags"]},
            "cards": cards, "reviews": reviews, "expected_absent": True}


@unittest.skipIf(Collection is None, "needs Anki's Python")
class MergeEffectTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.col = Collection(str(Path(self.tmp.name) / "collection.anki2"))
        model = self.col.models.by_name("Basic")
        deck = self.col.decks.id("Merge")
        self.notes = []
        for front in ("本", "本"):
            note = self.col.new_note(model)
            note["Front"], note["Back"] = front, "book"
            self.col.add_note(note, deck)
            self.notes.append(note.id)
        # Study the duplicate so it has a review row and a schedule.
        card = self.col.get_note(self.notes[1]).cards()[0]
        self.col.db.execute(
            "insert into revlog (id,cid,usn,ease,ivl,lastIvl,factor,time,type) "
            "values (1700000000000, ?, -1, 3, 4, 1, 2500, 5000, 1)", card.id)
        self.col.db.execute("update cards set type=2, queue=2, ivl=4, reps=1, due=100 "
                            "where id = ?", card.id)
        self.model_id = model["id"]

    def tearDown(self):
        self.col.close()
        self.tmp.cleanup()

    def test_merged_note_is_deleted_then_recreated_with_its_ids_schedule_and_reviews(self):
        survivor, merged = self.notes
        observed = effects.observe_note(self.col, merged)
        body = {"note_id": merged, "expected_pre_digest": effects.content_digest(observed),
                "survivor_note_id": survivor, "survivor_model_name": "Basic"}
        payloads.validate_body("delete_merged_note", body, operation_id="x",
                               approved_digest="lab-jcs-v1:lab-merge-decision-v1:" + "a" * 64)
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MERGE_SURVIVOR_INVALID"):
            effects.preflight_delete_merged_note(
                self.col, dict(body, survivor_model_name="Linguist Vocabulary v3"))
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_PRECONDITION_FAILED"):
            effects.preflight_delete_merged_note(self.col, dict(body, expected_pre_digest="x"))
        evidence = inspection.inspect_note(self.col, str(merged))
        effects.preflight_delete_merged_note(self.col, body)
        effects.perform_delete_merged_note(self.col, body, None)
        self.assertIsNone(effects.observe_or_none(self.col, merged))
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_NOTE_MISSING"):
            effects.preflight_delete_merged_note(self.col, body)

        restore = recreate_body(evidence, self.model_id)
        payloads.validate_body("recreate_deleted_note", restore, operation_id="x",
                               approved_digest="lab-jcs-v1:lab-merge-decision-v1:" + "b" * 64)
        effects.preflight_recreate_deleted_note(self.col, restore)
        effects.perform_recreate_deleted_note(self.col, restore, None)
        again = inspection.inspect_note(self.col, str(merged))
        self.assertEqual(again["note"]["fields"], evidence["note"]["fields"])
        self.assertEqual(again["note"]["guid"], evidence["note"]["guid"])
        for before, after in zip(evidence["cards"], again["cards"]):
            for key in ("id", "ordinal", "deck_id", "type", "queue", "due", "interval",
                        "ease_factor", "repetitions", "lapses"):
                self.assertEqual(before[key], after[key], key)
            self.assertEqual(before["reviews"], after["reviews"])
        # The checksum and sort field are recomputed: Anki finds the note.
        self.assertEqual(self.col.find_notes('"Front:本"'), sorted(self.notes))
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_NOTE_EXISTS"):
            effects.preflight_recreate_deleted_note(self.col, restore)


if __name__ == "__main__":
    unittest.main()
