# SPDX-License-Identifier: GPL-3.0-or-later
"""Exact typed bodies for every lab-native-v1 mutation variant."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge import manifest  # noqa: E402
from linguist_bridge.payloads import (  # noqa: E402
    PayloadError, VOCAB_FIELDS, VOCAB_V3_FIELDS, validate_body)

PLAN = "lab-jcs-v1:plan:" + "a" * 64
PRE = "lab-jcs-v1:lab-apply-precondition-v1:" + "b" * 64
OPERATION = "00000000-0000-4000-8000-000000000001"


KIND = {"export_checkpoint": "checkpoint", "install_model": "model-install",
        "restore_note": "lab-restore-decision-v1",
        "delete_unstudied_created_note": "lab-restore-decision-v1"}


def check(variant, body):
    approved = f"lab-jcs-v1:{KIND.get(variant, 'plan')}:" + "a" * 64
    validate_body(variant, body, operation_id=OPERATION, approved_digest=approved)


class PayloadTest(unittest.TestCase):
    def assertInvalid(self, variant, body):
        with self.assertRaisesRegex(PayloadError, "BRIDGE_OPERATION_BODY_INVALID"):
            check(variant, body)

    def test_update_and_restore_bodies_are_exact(self):
        migration = {"source_model_id": 5, "target_model_id": 6,
                     "target_model_name": "Linguist Vocabulary v2",
                     "ordinal_map": [{"source": 0, "target": 0}, {"source": 1, "target": 2}]}
        update = {"note_id": 7, "expected_pre_digest": PRE, "migration": migration,
                  "fields": {"Expression": "x"}, "add_tags": ["revamped"], "deck_id": 9}
        check("update_note", update)
        check("update_note", dict(update, migration=None))
        for edit in ({"note_id": "7"}, {"expected_pre_digest": "b" * 64}, {"deck_id": 0},
                     {"add_tags": ["two words"]}, {"fields": {}},
                     {"migration": dict(migration, ordinal_map=[{"source": 0, "target": 0},
                                                                {"source": 1, "target": 0}])}):
            self.assertInvalid("update_note", dict(update, **edit))
        self.assertInvalid("update_note", dict(update, extra=1))
        restore = {"note_id": 7, "expected_pre_digest": PRE, "migration": None,
                   "fields": {"Front": "a"}, "tags": [], "card_decks": [
                       {"card_id": 11, "deck_id": 1}], "removed_card_ids": [12]}
        check("restore_note", restore)
        self.assertInvalid("restore_note", dict(restore, removed_card_ids=[11]))
        self.assertInvalid("restore_note", dict(restore, card_decks=[]))
        check("delete_unstudied_created_note", {"note_id": 7, "expected_pre_digest": PRE})
        self.assertInvalid("delete_unstudied_created_note", {"note_id": 7})

    def test_v3_vocabulary_create_body_has_no_language_or_cue_fields(self):
        marker = "lab_op_" + OPERATION.replace("-", "")
        fields = {name: "" for name in VOCAB_V3_FIELDS}
        fields.update(Expression="猫", Meaning="<ol><li>cat</li></ol>", EnableSpelling="1")
        body = {"model_name": "Linguist Vocabulary v3", "model_manifest_digest": "b" * 64,
                "deck_id": "123", "fields": fields, "tags": [marker, "lab::lang::ja"],
                "marker_tag": marker, "source_plan_digest": PLAN, "checkpoint_digest": "c" * 64,
                "binding": {"profile_fingerprint": "d" * 64, "path_fingerprint": "e" * 64},
                "expected_absent": True}
        check("create_note", body)
        self.assertInvalid("create_note", dict(body, fields=dict(fields, Language="ja")))
        self.assertInvalid("create_note", dict(body, fields=dict(fields, EnableProduction="y")))
        self.assertInvalid("create_note", dict(body, fields=dict(fields, Meaning=" ")))
        missing = dict(fields)
        missing.pop("Kanji")
        self.assertInvalid("create_note", dict(body, fields=missing))

    def test_media_model_and_export_bodies(self):
        media = {"filename": "eat.ogg", "sha256": "c" * 64, "size_bytes": 10,
                 "staged_asset": "c" * 64}
        check("store_media", media)
        for name in ("../x", ".hidden", "a/b", "", "c:d", "a ", "x*"):
            self.assertInvalid("store_media", dict(media, filename=name))
        self.assertInvalid("store_media", dict(media, staged_asset="d" * 64))
        self.assertInvalid("store_media", dict(media, size_bytes=-1))
        model = {"name": "Linguist Vocabulary v2", "version": 2, "fields": sorted(VOCAB_FIELDS),
                 "templates": [{"name": "Recognition", "ordinal": 0, "front": "{{Expression}}",
                                "back": "{{Meaning}}"}], "css": ""}
        install = {"manifest": model, "manifest_digest": manifest.managed_digest(model),
                   "expected_absent": True}
        check("install_model", install)
        self.assertInvalid("install_model", dict(install, manifest=dict(model, name="Basic")))
        self.assertInvalid("install_model", dict(install, manifest=dict(
            model, templates=[dict(model["templates"][0], ordinal=1)])))
        self.assertInvalid("install_model", dict(install, expected_absent=False))
        check("export_checkpoint", {"include_media": True, "include_scheduling": True})
        self.assertInvalid("export_checkpoint", {"include_media": True,
                                                 "include_scheduling": False})
        self.assertInvalid("export_checkpoint", {"include_media": True,
                                                 "include_scheduling": True,
                                                 "path": "/tmp/x.colpkg"})
        with self.assertRaisesRegex(PayloadError, "BRIDGE_OPERATION_VARIANT_UNAVAILABLE"):
            check("run_sql", {})
        # Each variant accepts only its own approval kind.
        with self.assertRaisesRegex(PayloadError, "BRIDGE_OPERATION_BODY_INVALID"):
            validate_body("export_checkpoint", {"include_media": True, "include_scheduling": True},
                          operation_id=OPERATION, approved_digest=PLAN)
        with self.assertRaisesRegex(PayloadError, "BRIDGE_OPERATION_BODY_INVALID"):
            validate_body("install_model", install, operation_id=OPERATION,
                          approved_digest="lab-jcs-v1:checkpoint:" + "a" * 64)

    def test_manifest_digest_ignores_version_and_follows_template_ordinals(self):
        model = {"name": "M", "version": 2, "fields": ["A", "B"], "css": "x", "templates": [
            {"name": "T2", "ordinal": 1, "front": "f2", "back": "b2"},
            {"name": "T1", "ordinal": 0, "front": "f1", "back": "b1"}]}
        anki = {"name": "M", "css": "x", "flds": [{"name": "B", "ord": 1}, {"name": "A", "ord": 0}],
                "tmpls": [{"name": "T1", "ord": 0, "qfmt": "f1", "afmt": "b1"},
                          {"name": "T2", "ord": 1, "qfmt": "f2", "afmt": "b2"}]}
        self.assertEqual(manifest.managed_digest(model), manifest.model_digest(anki))
        self.assertEqual(manifest.managed_digest(dict(model, version=3)),
                         manifest.managed_digest(model))
        self.assertNotEqual(manifest.managed_digest(dict(model, fields=["B", "A"])),
                            manifest.managed_digest(model))
        # Shared vector with linguist_core::model::manifest_digest.
        self.assertEqual(manifest.digest(manifest.projection(
            "Linguist Vocabulary v2", ["Expression", "Meaning"],
            [{"name": "Recognition", "ordinal": 0, "front": "{{Expression}}", "back": "{{Meaning}}"}],
            ".card { color: black; }")),
            "3ebd381fb2210421a3a1be2416450552fa26f2282d5eef997ad2340e903fc029")


if __name__ == "__main__":
    unittest.main()
