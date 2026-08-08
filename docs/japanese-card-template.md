# Japanese vocabulary card template

`Linguist Japanese Vocabulary` is the Japanese-first note type supplied by the
bridge. Each note creates three cards in a stable order:

1. **Comprehension** — Japanese expression → reading, meaning, and usage.
2. **Spelling** — pronunciation/reading → typed Japanese expression. Anki's
   typed-answer comparison highlights mistakes after reveal.
3. **Production** — picture → spoken Japanese expression.

## Review layout

- Comprehension front: the Japanese expression only, so the reading and meaning
  are not exposed before recall.
- Spelling front: audio/reading plus a typed-answer box, without the expression.
- Production front: image only, without the expression, meaning, or audio.
- Every back: expression, reading and replay button, optional image, parsed dictionary
  meaning with LLM nuance/examples, then optional kanji construction and stroke
  order images.
- Styling: responsive desktop/mobile layout, Anki night-mode colors, bounded
  media, Japanese font fallbacks, and visually separate generated annotations.

## Fields

| Bridge purpose | Anki field |
| --- | --- |
| Expression | `Expression` |
| Meaning image | `Picture` |
| Meaning text | `Meaning` |
| Kanji construction | `Kanji` |
| Pronunciation | `Audio` |

Install it while Anki and AnkiConnect are running:

```bash
linguist-anki-bridge --install-japanese-template
```

Then select `Linguist Japanese Vocabulary` and the field mapping above in
Settings → Decks & Fields → Japanese Vocabulary.

Re-running the installer upgrades the former one-card managed note type safely:
it renames its ordinal-zero card to **Comprehension** and adds **Spelling** and
**Production**, preserving the existing card's scheduling. It refuses to
delete unexpected templates. Install or refresh the template before automatic
legacy-note migration; commits are blocked when the managed template still has
the obsolete one-card shape.
