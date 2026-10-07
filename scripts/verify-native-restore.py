#!/usr/bin/env python3
"""WP-12 disposable native restore probe.

Pipe `linguist-anki-bridge --output json models builtin` to stdin. The script
creates a collection in a temporary directory, never opens a user profile and
exercises `addons/linguist_bridge/effects.py` directly on Anki's backend:
restore after later study (fields, tags, per-card decks), reverse mapped
note-type change that keeps retained card IDs, scheduling and history while
removing only the listed unstudied new-task card, refusals for studied
new-task cards, reverse-mapping mismatches, stale preconditions and filtered
decks, created-note deletion only while unstudied, and survival of shared note
types and media. It does not exercise AnkiConnect, the companion ledger, the
Rust transport or the main-thread critical section.
"""

import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile

from anki.buildinfo import buildhash, version
from anki.collection import Collection

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "addons"))
from linguist_bridge import effects  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "verify_native_apply", ROOT / "scripts" / "verify-native-apply.py")
apply_probe = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(apply_probe)
manifest_digest = apply_probe.manifest_digest
install = apply_probe.install
study = apply_probe.study
vocab_fields = apply_probe.vocab_fields
expect_error = apply_probe.expect_error


def card(observed, card_id):
    return next(c for c in observed["cards"] if c["id"] == card_id)


def same_history(a, b):
    """Identity, scheduling and review history are identical."""
    return (a["id"] == b["id"] and a["scheduler"] == b["scheduler"]
            and a["history_digest"] == b["history_digest"]
            and a["review_count"] == b["review_count"])


def main():
    assert (version, buildhash) == ("25.09.2", "3d813c83"), (version, buildhash)
    manifests = json.load(sys.stdin)
    vocab_manifest = next(m for m in manifests if m["name"] == "Linguist Vocabulary v3")
    with tempfile.TemporaryDirectory(prefix="lab-native-restore-") as directory:
        col = Collection(str(Path(directory) / "disposable.anki2"))
        try:
            vocab = install(col, vocab_manifest)
            home = col.decks.id("Disposable home")
            target = col.decks.id("Disposable::Japanese::Vocab")

            # 1. Update restore after later study: content returns, history stays.
            note = col.new_note(vocab)
            original_fields = vocab_fields(vocab_manifest, Meaning="to consume")
            for name, value in original_fields.items():
                note[name] = value
            note.tags = ["old"]
            col.add_note(note, home)
            card_id = col.card_ids_of_note(note.id)[0]
            study(col, card_id)
            fresh = effects.observe_note(col, note.id, manifest_digest)
            effects.update_note(col, {
                "note_id": note.id, "expected_pre_digest": effects.content_digest(fresh),
                "migration": None,
                "fields": vocab_fields(vocab_manifest, EnableProduction="1"),
                "add_tags": ["linguist"], "deck_id": target,
            }, manifest_digest)
            study(col, card_id)  # later review after apply
            later = effects.observe_note(col, note.id, manifest_digest)
            new_card = next(c["id"] for c in later["cards"] if c["id"] != card_id)
            assert card(later, card_id)["review_count"] == 2
            effects.restore_note(col, {
                "note_id": note.id, "expected_pre_digest": effects.content_digest(later),
                "migration": None, "fields": original_fields, "tags": ["old"],
                "card_decks": [{"card_id": card_id, "deck_id": home},
                               {"card_id": new_card, "deck_id": target}],
                "removed_card_ids": [],
            }, manifest_digest)
            restored = effects.observe_note(col, note.id, manifest_digest)
            assert restored["fields"] == original_fields
            assert restored["tags"] == ["old"]
            assert same_history(card(later, card_id), card(restored, card_id))
            assert card(restored, card_id)["deck_id"] == home
            # The task card added by apply is kept with its own identity.
            assert card(restored, new_card)["deck_id"] == target
            assert card(restored, new_card)["review_count"] == 0

            # 2. Stale precondition: a user edit after the restore preview wins.
            preview = effects.observe_note(col, note.id, manifest_digest)
            user = col.get_note(note.id)
            user["Kanji"] = "user edit after preview"
            col.update_note(user)
            expect_error("BRIDGE_PRECONDITION_FAILED", effects.restore_note, col, {
                "note_id": note.id, "expected_pre_digest": effects.content_digest(preview),
                "migration": None, "fields": original_fields, "tags": ["old"],
                "card_decks": [{"card_id": card_id, "deck_id": home},
                               {"card_id": new_card, "deck_id": target}],
                "removed_card_ids": [],
            }, manifest_digest)
            assert col.get_note(note.id)["Kanji"] == "user edit after preview"

            # 3. Reverse mapped migration after later study.
            basic = col.models.by_name("Basic")
            source = col.new_note(basic)
            source["Front"] = "飲む"
            source["Back"] = "to drink"
            source.tags = ["old"]
            col.add_note(source, home)
            source_card = col.card_ids_of_note(source.id)[0]
            study(col, source_card)
            before = effects.observe_note(col, source.id, manifest_digest)
            forward = [{"source": 0, "target": 0}]
            effects.update_note(col, {
                "note_id": source.id, "expected_pre_digest": effects.content_digest(before),
                "migration": {"source_model_id": basic["id"], "target_model_id": vocab["id"],
                              "target_model_name": vocab["name"], "ordinal_map": forward},
                "fields": vocab_fields(vocab_manifest, Expression="飲む", Meaning="to drink",
                                       EnableProduction="1"),
                "add_tags": ["linguist"], "deck_id": target,
            }, manifest_digest)
            migrated = effects.observe_note(col, source.id, manifest_digest)
            assert migrated["model_id"] == vocab["id"]
            task_card = next(c["id"] for c in migrated["cards"] if c["id"] != source_card)
            study(col, source_card)  # later review on the retained card
            later = effects.observe_note(col, source.id, manifest_digest)
            reverse = {"source_model_id": vocab["id"], "target_model_id": basic["id"],
                       "target_model_name": "Basic",
                       "ordinal_map": [{"source": 0, "target": 0}]}
            body = {
                "note_id": source.id, "expected_pre_digest": effects.content_digest(later),
                "migration": reverse, "fields": {"Front": "飲む", "Back": "to drink"},
                "tags": ["old"], "card_decks": [{"card_id": source_card, "deck_id": home}],
                "removed_card_ids": [],
            }
            # The unstudied new-task card must be listed explicitly.
            expect_error("BRIDGE_REVERSE_MAPPING_MISMATCH", effects.restore_note, col, body,
                         manifest_digest)
            assert effects.observe_note(col, source.id, manifest_digest) == later
            effects.restore_note(col, dict(body, removed_card_ids=[task_card]),
                                 manifest_digest)
            back = effects.observe_note(col, source.id, manifest_digest)
            assert back["model_id"] == basic["id"]
            assert [c["id"] for c in back["cards"]] == [source_card]
            assert same_history(card(later, source_card), card(back, source_card))
            assert card(back, source_card)["review_count"] == 2
            assert card(back, source_card)["ordinal"] == 0
            assert card(back, source_card)["deck_id"] == home
            assert back["fields"] == {"Front": "飲む", "Back": "to drink"}
            assert col.get_card(source_card).note().id == source.id

            # 4. A studied new-task card blocks reverse mapping.
            other = col.new_note(basic)
            other["Front"] = "見る"
            other["Back"] = "to see"
            col.add_note(other, home)
            other_card = col.card_ids_of_note(other.id)[0]
            pre = effects.observe_note(col, other.id, manifest_digest)
            effects.update_note(col, {
                "note_id": other.id, "expected_pre_digest": effects.content_digest(pre),
                "migration": {"source_model_id": basic["id"], "target_model_id": vocab["id"],
                              "target_model_name": vocab["name"], "ordinal_map": forward},
                "fields": vocab_fields(vocab_manifest, Expression="見る", Meaning="to see",
                                       EnableProduction="1"),
                "add_tags": [], "deck_id": target,
            }, manifest_digest)
            expanded = effects.observe_note(col, other.id, manifest_digest)
            studied_task = next(c["id"] for c in expanded["cards"] if c["id"] != other_card)
            study(col, studied_task)
            current = effects.observe_note(col, other.id, manifest_digest)
            expect_error("BRIDGE_STUDIED_CARD_REMOVAL", effects.restore_note, col, {
                "note_id": other.id, "expected_pre_digest": effects.content_digest(current),
                "migration": reverse, "fields": {"Front": "見る", "Back": "to see"},
                "tags": [], "card_decks": [{"card_id": other_card, "deck_id": home}],
                "removed_card_ids": [studied_task],
            }, manifest_digest)
            assert effects.observe_note(col, other.id, manifest_digest) == current

            # 5. Filtered-deck membership blocks restore.
            filtered = col.decks.new_filtered("Disposable filtered")
            deck = col.decks.get(filtered)
            deck["terms"] = [[f"nid:{other.id}", 100, 0]]
            col.decks.save(deck)
            col.sched.rebuild_filtered_deck(filtered)
            pulled = effects.observe_note(col, other.id, manifest_digest)
            assert any(c["original_deck_id"] for c in pulled["cards"])
            expect_error("BRIDGE_FILTERED_DECK", effects.restore_note, col, {
                "note_id": other.id, "expected_pre_digest": effects.content_digest(pulled),
                "migration": None, "fields": pulled["fields"], "tags": [],
                "card_decks": [{"card_id": c["id"], "deck_id": home}
                               for c in pulled["cards"]],
                "removed_card_ids": [],
            }, manifest_digest)
            col.sched.empty_filtered_deck(filtered)

            # 6. Created notes: deleted only while unchanged and unstudied.
            data = b"OggS disposable created-note audio"
            digest = hashlib.sha256(data).hexdigest()
            media_name = f"lab-{digest[:16]}.ogg"
            effects.store_media(col, media_name, data, digest)
            created = []
            for index in range(2):
                marker = "lab_op_" + str(index) * 32
                created.append(effects.create_note(col, {
                    "model_name": vocab["name"], "model_manifest_digest": manifest_digest(vocab),
                    "deck_id": str(target),
                    "fields": vocab_fields(vocab_manifest, Expression=f"語{index}",
                                           Audio=f"[sound:{media_name}]"),
                    "tags": ["linguist", marker], "marker_tag": marker, "expected_absent": True,
                }, manifest_digest))
            study(col, col.card_ids_of_note(created[1])[0])
            for note_id, outcome in zip(created, ["deleted", "BRIDGE_STUDIED_NOTE"]):
                observed = effects.observe_note(col, note_id, manifest_digest)
                body = {"note_id": note_id,
                        "expected_pre_digest": effects.content_digest(observed)}
                if outcome == "deleted":
                    effects.delete_unstudied_created_note(col, body, manifest_digest)
                    assert not col.find_notes(f"nid:{note_id}")
                    expect_error("BRIDGE_NOTE_MISSING", effects.delete_unstudied_created_note,
                                 col, body, manifest_digest)
                else:
                    expect_error(outcome, effects.delete_unstudied_created_note, col, body,
                                 manifest_digest)
                    assert col.find_notes(f"nid:{note_id}") == [note_id]

            # 7. Shared note types and media survive every restore and deletion.
            assert col.models.by_name("Basic") is not None
            assert col.models.by_name(vocab["name"]) is not None
            assert effects.media_sha256(col, media_name) == digest
        finally:
            col.close()
    print("PASS: disposable native restore keeps retained card IDs, scheduling and later"
          " history through content restore and reverse mapping; studied cards and notes,"
          " stale preconditions, mapping mismatches and filtered decks are refused;"
          " shared note types and media survive")


if __name__ == "__main__":
    main()
