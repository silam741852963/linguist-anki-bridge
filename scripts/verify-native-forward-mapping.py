#!/usr/bin/env python3
"""Probe Basic and Picture Words forward card mappings in disposable Anki.

Pipe `linguist-anki-bridge models builtin` JSON to stdin. Never opens a user profile.
"""

import json
from pathlib import Path
import sys
import tempfile

from anki.buildinfo import buildhash, version
from anki.collection import Collection


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


def snapshot(col, card_id):
    card = col.get_card(card_id)
    history = col.db.all(
        "select id, cid, usn, ease, ivl, lastIvl, factor, time, type "
        "from revlog where cid = ? order by id", card_id)
    return {
        "id": card.id, "nid": card.nid, "ord": card.ord,
        "did": card.did, "odid": card.odid, "odue": card.odue,
        "queue": card.queue, "type": card.type, "due": card.due,
        "ivl": card.ivl,
        "factor": card.factor, "reps": card.reps, "lapses": card.lapses,
        "left": card.left, "flags": card.flags,
        "original_position": card.original_position,
        "custom_data": card.custom_data,
        "memory_state": str(card.memory_state),
        "desired_retention": card.desired_retention, "decay": card.decay,
        "last_review_time": card.last_review_time,
        "history": history,
    }


def verify_picture_words(col, vocab_manifest, deck_id):
    """Probe three-card ordinal retention using the known Picture Words field names."""
    fields = [
        "Word", "Picture", "Gender, Personal Connection, Extra Info (Back side)",
        "Pronunciation (Recording and/or IPA)",
        "Test Spelling? (y = yes, blank = no)",
    ]
    source = col.models.new("2. Picture Words (disposable)")
    for name in fields:
        col.models.add_field(source, col.models.new_field(name))
    for name, front in [
        ("Comprehension Card", "{{Word}}"),
        ("Production Card", "{{Picture}}"),
        ("Spelling?", "{{Pronunciation (Recording and/or IPA)}}"),
    ]:
        template = col.models.new_template(name)
        template["qfmt"] = front
        template["afmt"] = "{{Word}}"
        col.models.add_template(source, template)
    col.models.add_dict(source)
    source = col.models.by_name(source["name"])
    target = install(col, vocab_manifest)
    note = col.new_note(source)
    note["Word"] = "食べる"
    note["Picture"] = "picture cue"
    note["Gender, Personal Connection, Extra Info (Back side)"] = "to eat"
    note["Pronunciation (Recording and/or IPA)"] = "Write this"
    note["Test Spelling? (y = yes, blank = no)"] = "y"
    col.add_note(note, deck_id)
    originals = sorted((col.get_card(card_id) for card_id in col.card_ids_of_note(note.id)),
                       key=lambda card: card.ord)
    assert [card.ord for card in originals] == [0, 1, 2]
    originals[0].start_timer()
    col.sched.answerCard(originals[0], 3)
    before = {card.id: snapshot(col, card.id) for card in originals}
    assert len(before[originals[0].id]["history"]) == 1

    info = col.models.change_notetype_info(
        old_notetype_id=source["id"], new_notetype_id=target["id"])
    request = info.input
    request.note_ids.append(note.id)
    source_fields = {field["name"]: index for index, field in enumerate(source["flds"])}
    mapped = {
        "Expression": "Word",
        "Meaning": "Gender, Personal Connection, Extra Info (Back side)",
        "Picture": "Picture",
        "Pronunciation": "Pronunciation (Recording and/or IPA)",
        "EnableProduction": "Picture",
        "ProductionPrompt": "Picture",
        "EnableSpelling": "Test Spelling? (y = yes, blank = no)",
        "SpellingPrompt": "Pronunciation (Recording and/or IPA)",
    }
    del request.new_fields[:]
    request.new_fields.extend(source_fields[mapped[field["name"]]]
                              if field["name"] in mapped else -1
                              for field in target["flds"])
    del request.new_templates[:]
    request.new_templates.extend([0, 1, 2])
    col.models.change_notetype_of_notes(request)
    after = {card_id: snapshot(col, card_id) for card_id in col.card_ids_of_note(note.id)}
    assert after == before, (before, after)
    assert col.get_note(note.id).mid == target["id"]


def verify_studied_child_reverse_risk(col, grammar, basic, deck_id):
    note = col.new_note(grammar)
    for name, value in {
        "Pattern": "〜ても", "Meaning": "even if",
        "RecognitionPrompt": "Which pattern?", "EnableApplication": "1",
        "ExercisePrompt": "Complete this", "ExerciseAnswer": "〜ても",
    }.items():
        note[name] = value
    col.add_note(note, deck_id)
    cards = {card.ord: card for card in
             (col.get_card(card_id) for card_id in col.card_ids_of_note(note.id))}
    assert set(cards) == {0, 1}
    child = cards[1]
    child.start_timer()
    col.sched.answerCard(child, 3)
    assert len(snapshot(col, child.id)["history"]) == 1

    info = col.models.change_notetype_info(
        old_notetype_id=grammar["id"], new_notetype_id=basic["id"])
    request = info.input
    request.note_ids.append(note.id)
    fields = {field["name"]: index for index, field in enumerate(grammar["flds"])}
    del request.new_fields[:]
    request.new_fields.extend([fields["Pattern"], fields["Meaning"]])
    del request.new_templates[:]
    request.new_templates.extend([0])
    col.models.change_notetype_of_notes(request)
    card_ids = col.card_ids_of_note(note.id)
    child_row_count = col.db.scalar("select count(*) from cards where id = ?", child.id)
    child_history = col.db.all("select id from revlog where cid = ?", child.id)
    return child.id not in card_ids and child_row_count == 0, bool(child_history)


def main():
    assert (version, buildhash) == ("25.09.2", "3d813c83"), (version, buildhash)
    manifests = json.load(sys.stdin)
    grammar_manifest = next(item for item in manifests
                            if item["name"] == "Linguist Grammar v2")
    vocab_manifest = next(item for item in manifests
                          if item["name"] == "Linguist Vocabulary v3")
    with tempfile.TemporaryDirectory(prefix="lab-native-basic-migration-") as directory:
        col = Collection(str(Path(directory) / "disposable.anki2"))
        try:
            basic = col.models.by_name("Basic")
            assert basic is not None
            assert [field["name"] for field in basic["flds"]] == ["Front", "Back"]
            grammar = install(col, grammar_manifest)
            deck_id = col.decks.id("Disposable native migration")
            note = col.new_note(basic)
            note["Front"] = "〜ても"
            note["Back"] = "even if"
            col.add_note(note, deck_id)
            cards = col.card_ids_of_note(note.id)
            assert len(cards) == 1
            card_id = cards[0]
            card = col.get_card(card_id)
            card.start_timer()
            col.sched.answerCard(card, 3)
            before = snapshot(col, card_id)
            assert len(before["history"]) == 1 and before["reps"] == 1

            info = col.models.change_notetype_info(
                old_notetype_id=basic["id"], new_notetype_id=grammar["id"])
            request = info.input
            request.note_ids.append(note.id)
            source_fields = {field["name"]: index for index, field in enumerate(basic["flds"])}
            field_map = {
                "Pattern": source_fields["Front"],
                "Meaning": source_fields["Back"],
                "RecognitionPrompt": source_fields["Front"],
            }
            del request.new_fields[:]
            request.new_fields.extend(field_map.get(field["name"], -1)
                                      for field in grammar["flds"])
            del request.new_templates[:]
            request.new_templates.extend([0, -1])
            col.models.change_notetype_of_notes(request)

            after = snapshot(col, card_id)
            assert after == before, (before, after)
            mapped = col.get_note(note.id)
            assert mapped.mid == grammar["id"]
            assert mapped["Pattern"] == "〜ても"
            assert mapped["Meaning"] == "even if"
            assert mapped["RecognitionPrompt"] == "〜ても"
            assert col.card_ids_of_note(note.id) == [card_id]
            mapped["EnableApplication"] = "1"
            mapped["ExercisePrompt"] = "Finish the pattern"
            mapped["ExerciseAnswer"] = "〜ても"
            col.update_note(mapped)
            cards_after_expansion = sorted(
                (col.get_card(candidate) for candidate in col.card_ids_of_note(note.id)),
                key=lambda candidate: candidate.ord)
            assert [candidate.ord for candidate in cards_after_expansion] == [0, 1]
            assert cards_after_expansion[0].id == card_id
            assert cards_after_expansion[1].id != card_id
            assert snapshot(col, card_id) == before
            fresh = snapshot(col, cards_after_expansion[1].id)
            assert fresh["history"] == [] and fresh["reps"] == 0

            studied = col.get_card(card_id)
            studied.start_timer()
            col.sched.answerCard(studied, 3)
            before_reverse = snapshot(col, card_id)
            assert len(before_reverse["history"]) == 2
            reverse_info = col.models.change_notetype_info(
                old_notetype_id=grammar["id"], new_notetype_id=basic["id"])
            reverse = reverse_info.input
            reverse.note_ids.append(note.id)
            grammar_fields = {field["name"]: index
                              for index, field in enumerate(grammar["flds"])}
            del reverse.new_fields[:]
            reverse.new_fields.extend([grammar_fields["Pattern"], grammar_fields["Meaning"]])
            del reverse.new_templates[:]
            reverse.new_templates.extend([0])
            col.models.change_notetype_of_notes(reverse)
            assert snapshot(col, card_id) == before_reverse
            assert col.card_ids_of_note(note.id) == [card_id]
            restored = col.get_note(note.id)
            assert restored.mid == basic["id"]
            assert restored["Front"] == "〜ても" and restored["Back"] == "even if"
            verify_picture_words(col, vocab_manifest, deck_id)
            child_removed, history_retained = verify_studied_child_reverse_risk(
                col, grammar, basic, deck_id)
            assert child_removed, "unexpected retained studied Application card"
            assert history_retained, "unexpected loss of studied Application review rows"
        finally:
            col.close()
    print("PASS: disposable Basic forward/reverse and Picture Words forward mappings retain history")
    print(f"OBSERVED: reverse drops studied Application card; review rows retained={history_retained}")


if __name__ == "__main__":
    main()
