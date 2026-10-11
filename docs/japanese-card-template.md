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
delete unexpected templates. A commit or automatic legacy-note migration also
runs this safe upgrade automatically when it detects the known obsolete
`Japanese Recognition` shape. The explicit installer remains available for
setup and for diagnosing AnkiConnect installation errors.

## Legacy expressions and pronunciation tracks

Modernization splits a legacy note only when its expression field contains
multiple explicit HTML/text lines. For example, a pronoun note containing
`私`, `僕`, `俺`, `我`, `あたし`, and `自分` becomes six managed notes. Preview
shows the split position and accepts `n`/`p` or Left/Right while Preview is
focused. The first result reuses the original note ID so its scheduling remains
stable; the additional notes inherit the original tags and each produces the
same three managed card templates.

Multiple audio files alone never trigger a split. A single expression such as
`脅かす` therefore stays one note and retains both `おどかす` and `おびやかす`
as ordered pronunciation tracks in the Audio field.

The snapshot captured before commit records every created sibling note and all
tracked media. Reverting the word-level snapshot deletes those siblings,
restores the original model, fields, tags, and media, and leaves unrelated notes
untouched. A failed partial split performs the same compensation immediately.
