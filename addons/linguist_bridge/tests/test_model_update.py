# SPDX-License-Identifier: GPL-3.0-or-later
"""WP-27 update_model on a real (temporary) Anki collection: templates and
CSS change in place, the note type and its cards keep their ids, and the
schema is not marked changed (no full sync)."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge import effects, manifest  # noqa: E402

try:
    from anki.collection import Collection
except ImportError:  # pragma: no cover - plain Python without Anki
    Collection = None

FIELDS = ["Expression", "Pronunciation", "Meaning", "UsageExamples", "Picture", "Audio",
          "Kanji", "EnableProduction", "EnableSpelling"]


def target(css, front):
    return {"name": "Linguist Vocabulary v3", "version": 3, "fields": FIELDS, "css": css,
            "templates": [{"name": "Comprehension", "ordinal": 0, "front": front,
                           "back": "{{FrontSide}}<hr id=answer>{{Meaning}}"}]}


@unittest.skipIf(Collection is None, "needs Anki's Python")
class ModelUpdateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.col = Collection(str(Path(self.tmp.name) / "collection.anki2"))
        old = target(".card { color: pink; }", "{{Expression}}")
        self.model_id = effects.perform_install_model(
            self.col, {"manifest": old, "manifest_digest": manifest.managed_digest(old)}, None)
        note = self.col.new_note(self.col.models.get(self.model_id))
        note["Expression"], note["Meaning"] = "食べる", "eat"
        note.tags = ["lab::pos::verb"]
        self.col.add_note(note, self.col.decks.id("Update"))
        self.note_id = note.id
        self.card_ids = sorted(self.col.card_ids_of_note(note.id))
        self.prior = manifest.model_digest(self.col.models.get(self.model_id))

    def tearDown(self):
        self.col.close()
        self.tmp.cleanup()

    def test_templates_and_css_change_in_place(self):
        new = target(".lab-card[data-tags~=\"lab::pos::verb\"] { color: red; }",
                     '<div class="lab-card" data-tags="{{Tags}}">{{Expression}}</div>')
        body = {"manifest": new, "manifest_digest": manifest.managed_digest(new),
                "model_id": self.model_id, "expected_prior_digest": self.prior}
        schema = self.col.db.scalar("select scm from col")
        effects.preflight_update_model(self.col, body)
        self.assertEqual(effects.perform_update_model(self.col, body, None), self.model_id)
        model = self.col.models.get(self.model_id)
        self.assertEqual(manifest.model_digest(model), body["manifest_digest"])
        self.assertEqual(self.col.db.scalar("select scm from col"), schema)
        self.assertEqual(sorted(self.col.card_ids_of_note(self.note_id)), self.card_ids)
        card = self.col.get_card(self.card_ids[0])
        self.assertIn('data-tags="lab::pos::verb"', card.question())
        # Applied once, the prior manifest is gone: a second update is refused.
        with self.assertRaisesRegex(effects.EffectError, "BRIDGE_MODEL_CHANGED"):
            effects.preflight_update_model(self.col, body)


if __name__ == "__main__":
    unittest.main()
