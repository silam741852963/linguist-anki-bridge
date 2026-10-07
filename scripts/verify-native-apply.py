#!/usr/bin/env python3
"""WP-11 disposable native apply probe.

Pipe `linguist-anki-bridge --output json models builtin` to stdin. The script
creates a collection in a temporary directory, never opens a user profile and
exercises `addons/linguist_bridge/effects.py` directly on Anki's backend:
marker creation, fresh scheduler capture after study, precondition CAS,
mapped migration, deck moves, task expansion, media no-overwrite and the
filtered-deck block. It does not exercise AnkiConnect, the companion ledger,
the Rust transport or the main-thread critical section.
"""

import hashlib
import json
from pathlib import Path
import sys
import tempfile

from anki.buildinfo import buildhash, version
from anki.collection import Collection

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "addons"))
from linguist_bridge import effects  # noqa: E402


def manifest_digest(model):
    """Deterministic stand-in for the capture-side model manifest digest."""
    projection = {
        "name": model["name"],
        "fields": [field["name"] for field in model["flds"]],
        "templates": [[t["name"], t["ord"], t["qfmt"], t["afmt"]] for t in model["tmpls"]],
        "css": model["css"],
    }
    return hashlib.sha256(effects.jcs(projection)).hexdigest()


def install(col, manifest):
    model = col.models.new(manifest["name"])
    model["css"] = manifest["css"]
    for name in manifest["fields"]:
        col.models.add_field(model, col.models.new_field(name))
    for template in manifest["templates"]:
        item = col.models.new_template(template["name"])
        item["qfmt"] = template["front"]
        item["afmt"] = template["back"]
        col.models.add_template(model, item)
    col.models.add_dict(model)
    return col.models.by_name(manifest["name"])


def study(col, card_id):
    card = col.get_card(card_id)
    card.start_timer()
    col.sched.answerCard(card, 3)


def vocab_fields(manifest, **values):
    fields = {name: "" for name in manifest["fields"]}
    fields.update({"Expression": "食べる", "Meaning": "to eat"})
    fields.update(values)
    return fields


def retained(before, after, card_id):
    """Card identity, scheduling and history unchanged apart from placement."""
    a = next(card for card in before["cards"] if card["id"] == card_id)
    b = next(card for card in after["cards"] if card["id"] == card_id)
    return (a["scheduler"] == b["scheduler"] and a["history_digest"] == b["history_digest"]
            and a["review_count"] == b["review_count"])


def expect_error(code, function, *args):
    try:
        function(*args)
    except effects.EffectError as error:
        assert str(error) == code, (code, error)
        return
    raise AssertionError(f"expected {code}")


def main():
    assert (version, buildhash) == ("25.09.2", "3d813c83"), (version, buildhash)
    manifests = json.load(sys.stdin)
    vocab_manifest = next(m for m in manifests if m["name"] == "Linguist Vocabulary v3")
    with tempfile.TemporaryDirectory(prefix="lab-native-apply-") as directory:
        col = Collection(str(Path(directory) / "disposable.anki2"))
        try:
            vocab = install(col, vocab_manifest)
            home = col.decks.id("Disposable home")
            target = col.decks.id("Disposable::Japanese::Vocab")

            # 1. Marker creation and expected-absent precondition.
            marker = "lab_op_" + "a" * 32
            body = {
                "model_name": vocab["name"], "model_manifest_digest": manifest_digest(vocab),
                "deck_id": str(target), "fields": vocab_fields(vocab_manifest),
                "tags": ["linguist", marker], "marker_tag": marker, "expected_absent": True,
            }
            created = effects.create_note(col, body, manifest_digest)
            assert col.find_notes(f"tag:{marker}") == [created]
            observed = effects.observe_note(col, created, manifest_digest)
            assert [c["ordinal"] for c in observed["cards"]] == [0]
            assert observed["cards"][0]["deck_id"] == target
            assert observed["cards"][0]["review_count"] == 0
            expect_error("BRIDGE_PRECONDITION_FAILED", effects.create_note, col, body,
                         manifest_digest)
            assert len(col.find_notes(f"tag:{marker}")) == 1

            # 2. Study between preparation and apply; fresh capture; update.
            note = col.new_note(vocab)
            for name, value in vocab_fields(vocab_manifest, Meaning="to consume").items():
                note[name] = value
            note.tags = ["old"]
            col.add_note(note, home)
            card_id = col.card_ids_of_note(note.id)[0]
            study(col, card_id)
            prepared = effects.observe_note(col, note.id, manifest_digest)
            study(col, card_id)  # normal review after preparation
            fresh = effects.observe_note(col, note.id, manifest_digest)
            assert effects.content_digest(prepared) == effects.content_digest(fresh)
            assert fresh["cards"][0]["review_count"] == 2
            update = {
                "note_id": note.id, "expected_pre_digest": effects.content_digest(fresh),
                "migration": None,
                "fields": vocab_fields(vocab_manifest, EnableProduction="1"),
                "add_tags": ["linguist"], "deck_id": target,
            }
            effects.update_note(col, update, manifest_digest)
            after = effects.observe_note(col, note.id, manifest_digest)
            assert retained(fresh, after, card_id), (fresh, after)
            assert sorted(c["ordinal"] for c in after["cards"]) == [0, 1]
            assert all(c["deck_id"] == target for c in after["cards"])
            new_card = next(c for c in after["cards"] if c["id"] != card_id)
            assert new_card["review_count"] == 0
            assert set(after["tags"]) == {"old", "linguist"}
            assert after["fields"]["Meaning"] == "to eat"

            # 3. Source edit after capture: CAS refuses, note untouched.
            edited = effects.observe_note(col, note.id, manifest_digest)
            user = col.get_note(note.id)
            user["Meaning"] = "user edit"
            col.update_note(user)
            stale = dict(update, expected_pre_digest=effects.content_digest(edited))
            expect_error("BRIDGE_PRECONDITION_FAILED", effects.update_note, col, stale,
                         manifest_digest)
            assert col.get_note(note.id)["Meaning"] == "user edit"

            # 4. Mapped migration Basic -> managed vocabulary retains the studied card.
            basic = col.models.by_name("Basic")
            source = col.new_note(basic)
            source["Front"] = "飲む"
            source["Back"] = "to drink"
            col.add_note(source, home)
            source_card = col.card_ids_of_note(source.id)[0]
            study(col, source_card)
            before = effects.observe_note(col, source.id, manifest_digest)
            migrate = {
                "note_id": source.id, "expected_pre_digest": effects.content_digest(before),
                "migration": {
                    "source_model_id": basic["id"], "target_model_id": vocab["id"],
                    "target_model_name": vocab["name"],
                    "ordinal_map": [{"source": 0, "target": 0}],
                },
                "fields": vocab_fields(vocab_manifest, Expression="飲む", Meaning="to drink"),
                "add_tags": ["linguist"], "deck_id": target,
            }
            effects.update_note(col, migrate, manifest_digest)
            migrated = effects.observe_note(col, source.id, manifest_digest)
            assert migrated["model_id"] == vocab["id"]
            assert [c["id"] for c in migrated["cards"]] == [source_card]
            assert retained(before, migrated, source_card), (before, migrated)
            assert migrated["cards"][0]["deck_id"] == target

            # 5. Filtered-deck membership blocks the update.
            filtered = col.decks.new_filtered("Disposable filtered")
            deck = col.decks.get(filtered)
            deck["terms"] = [[f"nid:{source.id}", 100, 0]]
            col.decks.save(deck)
            col.sched.rebuild_filtered_deck(filtered)
            in_filter = effects.observe_note(col, source.id, manifest_digest)
            assert in_filter["cards"][0]["original_deck_id"] == target
            again = dict(migrate, migration=None,
                         expected_pre_digest=effects.content_digest(in_filter))
            expect_error("BRIDGE_FILTERED_DECK", effects.update_note, col, again,
                         manifest_digest)
            col.sched.empty_filtered_deck(filtered)

            # 6. Media: verified store, identical reuse, collision never overwrites.
            data = b"OggS disposable apply audio"
            digest = hashlib.sha256(data).hexdigest()
            name = f"lab-{digest[:16]}.ogg"
            assert effects.store_media(col, name, data, digest) == name
            assert effects.store_media(col, name, data, digest) == name
            other = b"different bytes"
            expect_error("BRIDGE_MEDIA_COLLISION", effects.store_media, col, name, other,
                         hashlib.sha256(other).hexdigest())
            assert effects.media_sha256(col, name) == digest
            # 7. FSRS: a deck move keeps the memory state (Anki's set_deck
            # clears it); a move into a deck with another preset is refused.
            col.set_config("fsrs", True)
            fsrs = col.new_note(vocab)
            for name, value in vocab_fields(vocab_manifest, Expression="走る").items():
                fsrs[name] = value
            col.add_note(fsrs, home)
            fsrs_card = col.card_ids_of_note(fsrs.id)[0]
            study(col, fsrs_card)
            study(col, fsrs_card)
            before = effects.observe_note(col, fsrs.id, manifest_digest)
            assert before["cards"][0]["scheduler"]["memory_state"] != "None"
            move = {
                "note_id": fsrs.id, "expected_pre_digest": effects.content_digest(before),
                "migration": None, "fields": vocab_fields(vocab_manifest, Expression="走る"),
                "add_tags": [], "deck_id": target,
            }
            effects.update_note(col, move, manifest_digest)
            moved = effects.observe_note(col, fsrs.id, manifest_digest)
            assert retained(before, moved, fsrs_card), (before, moved)
            assert moved["cards"][0]["deck_id"] == target
            other_preset = col.decks.add_config_returning_id("Disposable other preset")
            other_deck = col.decks.id("Disposable other preset deck")
            deck = col.decks.get(other_deck)
            deck["conf"] = other_preset
            col.decks.save(deck)
            refused = dict(move, deck_id=other_deck,
                           expected_pre_digest=effects.content_digest(moved))
            expect_error("BRIDGE_FSRS_PRESET_CHANGE", effects.update_note, col, refused,
                         manifest_digest)
            assert effects.observe_note(col, fsrs.id, manifest_digest) == moved
        finally:
            col.close()
    print("PASS: disposable native apply effects retain card IDs, history, scheduling and"
          " FSRS memory state; CAS, filtered-deck, preset-change and media collision"
          " refusals verified")


if __name__ == "__main__":
    main()
