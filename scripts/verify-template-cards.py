#!/usr/bin/env python3
"""Verify managed template card ordinals in a disposable Anki collection.

Pipe the JSON output of `linguist-anki-bridge models builtin` to this script.
Requires the Anki Python package; it never opens a user profile.
"""

import json
import sys
import tempfile
from pathlib import Path

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
    installed = col.models.by_name(manifest["name"])
    assert installed is not None
    assert [field["name"] for field in installed["flds"]] == manifest["fields"]
    assert [template["name"] for template in installed["tmpls"]] == [
        template["name"] for template in manifest["templates"]
    ]
    assert installed["css"] == manifest["css"]
    for ordinal, template in enumerate(manifest["templates"]):
        assert installed["tmpls"][ordinal]["ord"] == ordinal
        assert installed["tmpls"][ordinal]["qfmt"] == template["front"]
        assert installed["tmpls"][ordinal]["afmt"] == template["back"]
    return installed


def check(col, model, deck_id, fields, expected):
    note = col.new_note(model)
    for name, value in fields.items():
        note[name] = value
    col.add_note(note, deck_id)
    cards = [col.get_card(card_id) for card_id in col.find_cards(f"nid:{note.id}")]
    actual = sorted(card.ord for card in cards)
    assert actual == expected, (model["name"], fields, actual, expected)
    return cards


def main():
    manifests = json.load(sys.stdin)
    assert [model["name"] for model in manifests] == [
        "Linguist Vocabulary v3",
        "Linguist English Vocabulary v1",
        "Linguist Grammar v4",
    ]
    with tempfile.TemporaryDirectory(prefix="lab-template-cards-") as directory:
        col = Collection(str(Path(directory) / "disposable.anki2"))
        try:
            deck_id = col.decks.id("Disposable template test")
            vocab, english, grammar = [install(col, manifest) for manifest in manifests]
            # v3: fronts show fields; the answer never appears on a task front.
            vbase = {"Expression": "食べる", "Meaning": "to eat"}
            check(col, vocab, deck_id, vbase, [0])
            production = {**vbase, "EnableProduction": "1", "Picture": '<img src="x.png">'}
            production_cards = check(col, vocab, deck_id, production, [0, 1])
            question = production_cards[1].question()
            assert "to eat" in question and "x.png" in question and "食べる" not in question
            spelling = {**vbase, "EnableSpelling": "1", "Pronunciation": "たべる"}
            spelling_cards = check(col, vocab, deck_id, spelling, [0, 2])
            question = spelling_cards[1].question()
            assert "たべる" in question and "to eat" in question and "食べる" not in question
            assert "食べる" in spelling_cards[1].answer()
            check(col, vocab, deck_id, {**production, **spelling}, [0, 1, 2])
            # English: same fronts, no Kanji section.
            ebase = {"Expression": "supernova", "Meaning": "a stellar explosion",
                     "EnableProduction": "1", "EnableSpelling": "1", "Pronunciation": "/ˌsupɚˈnoʊvə/"}
            english_cards = check(col, english, deck_id, ebase, [0, 1, 2])
            assert "supernova" not in english_cards[1].question()
            assert "supernova" not in english_cards[2].question() and "/ˌsupɚˈnoʊvə/" in english_cards[2].question()

            # Grammar v4: Recognition shows the pattern only (an example would
            # bring its sound to the front); the back lists every example with
            # its own play button, then Usage and Nuance. No Application card.
            gbase = {
                "Pattern": "〜ても",
                "Meaning": '<div class="lab-gloss">even if</div>',
                "Formation": "V-て + も",
                "Example": '<ul class="lab-examples"><li class="lab-voiced">'
                           '<div class="lab-example-line">[sound:a.wav]<span class="lab-target">'
                           '雨が降っ<b class="lab-hl">ても</b>行きます。</span></div></li>'
                           '<li class="lab-voiced"><div class="lab-example-line">[sound:b.wav]'
                           '<span class="lab-target">高く<b class="lab-hl">ても</b>買います。</span>'
                           '</div></li></ul>',
                "Usage": "<p>Concession.</p>",
                "Nuance": "<dl class=\"lab-nuance\"><div><dt>〜のに</dt><dd>Regret.</dd></div></dl>",
            }
            recognition = check(col, grammar, deck_id, gbase, [0])
            question = recognition[0].question()
            assert "〜ても" in question and "雨が降っ" not in question and "even if" not in question
            assert "[anki:play" not in question
            answer = recognition[0].answer()
            assert "even if" in answer and "Concession." in answer and "〜のに" in answer
            assert answer.count("[anki:play:a:") == 2, answer
            assert len(grammar["tmpls"]) == 1
        finally:
            col.close()
    print("PASS: disposable Anki model/template card ordinals and optional front gates")


if __name__ == "__main__":
    main()
