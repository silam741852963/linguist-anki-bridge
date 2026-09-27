# Research reference

## 1. Read-only collection grounding

Evidence: [aggregate collection inventory](../anki-inventory-2026-09-26.json). Queried AnkiConnect at `http://127.0.0.1:8765`, protocol 6. Read actions included version, deck/model names, model fields/templates/styling, indexed note/card search, paged `notesInfo`, bounded `cardsInfo`, active-profile name, action reflection and two source-image retrievals. No note/media/model/deck mutation, backup, import, model generation, or profile switch was invoked.

The persisted inventory contains aggregate counts/schema names, not note text, media payloads, raw profile names, or card IDs. Image samples were downloaded only to `/tmp` for inspection. Collection-wide model counts are exact for that inspection; deck counts reflect Anki's deck queries and need not form disjoint partitions in arbitrary collections. Content indicators establish markup presence, not media validity or semantic quality.

| Deck | Selected notes | Cards | Observed model | Images present | Audio present |
| --- | ---: | ---: | --- | ---: | ---: |
| Moonlit Manuscripts | 6,161 | 18,480 | Picture Words | 6,161 | 5,986 |
| 森の言葉 | 12,270 | 36,737 | 12,260 Picture Words; 10 Basic | 11,887 | 11,733 |
| 夕暮れの詞 | 544 | 544 | Vietnamese-localized Basic | 544 | 0 |
| Custom Study Session | 0 | 0 | — | 0 | 0 |
| Mặc định | 0 | 0 | — | 0 | 0 |

Collection model totals: **18,421 Picture Words notes and 554 Basic notes**, totaling **18,975 notes**. Observed deck card totals sum to **55,761**. Twelve note types exist; ten currently have no notes. `Linguist Japanese Vocabulary` has no notes and only the historical `Japanese Recognition` template. Neither the managed English vocabulary nor Japanese grammar model is installed.

Bounded content samples support the working interpretation Moonlit Manuscripts → English vocabulary, 森の言葉 → Japanese vocabulary, 夕暮れの詞 → Japanese grammar. These are **inferences**, not language declarations made by Anki. The ten Basic notes inside the vocabulary deck require per-note classification; the CLI must not classify them solely by deck membership.


Subsections:

- [1.1 What the legacy cards actually require](01-01-11-what-the-legacy-cards-actually-require.md)
- [1.2 Live API blockers and selected resolution](01-02-12-live-api-blockers-and-selected-resolution.md)
