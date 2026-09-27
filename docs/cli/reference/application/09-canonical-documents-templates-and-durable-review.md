# Application specification reference

## 9. Canonical documents, templates, and durable review

The v1 document/model shapes below describe compatibility/current code. New implementation uses the researched plan's **LearningDocument v2** vocabulary/grammar union and new **Linguist Vocabulary v2 / Linguist Grammar v2** model families. Version this change explicitly; do not mutate old field-order contracts in place or assume rich grammar/source objects fit the current v1 struct. Vocabulary template order becomes Comprehension, Production, Spelling to match legacy semantic tasks, with explicit native mappings. The v2 grammar standard includes Recognition and optional Application. Full field lists, task defaults, prerequisites and source preservation are defined in the researched plan.

Existing `CardDocument` v1 contains `schema_version`, `expression`, logical values (`meaning_image`, `meaning_text`, optional `examples`, `kanji_construction`, `audio`), media assets, obsolete media, issues, tags, and provenance. Logical `null` means no supplied update, while `""` is an explicit empty value; preserve this distinction through review, mapping, export, and apply. The existing mapper concatenates shared physical fields deterministically.

Vocabulary dictionary HTML and LLM annotations remain separate source-owned sections. Grammar explanation and examples can map separately. User Markdown is rendered with raw HTML disabled and trusted media placeholders preserved. Rust currently has no equivalent full Markdown authoring implementation in its core builder: **REVIEW R23** covers editor format and porting expectations.

Managed models currently defined:

| Purpose | Fields in order | Card templates |
| --- | --- | --- |
| Japanese vocabulary | Expression, Picture, Meaning, Kanji, Audio | Comprehension, Spelling, Production |
| English vocabulary | Expression, Picture, Meaning, Audio | Comprehension, Spelling, Production |
| Japanese grammar | Expression, Explanation, Examples | Recognition |

Comprehension reveals meaning/supporting material after recall; Spelling provides an audio/reading prompt and typed answer; Production uses the picture prompt. Do not accidentally expose answers on fronts. Missing-picture/audio gating and card creation behavior must be checked in real Anki, not inferred from raw template text: **REVIEW R24**.

Install/refresh only recognized managed schemas. Reject unexpected field order or templates rather than overwriting an unrelated model. Preserve ordinal-zero Japanese cards when upgrading the historical `Japanese Recognition` template; do not remove unexpected templates because that may delete scheduled cards. Display shared-model changes because they affect notes outside the selected plan.

Migrate supported legacy notes in place only when the verified native adapter supports explicit field/card-template mapping on the installed Anki build. The installed custom `updateNoteModel` action is not sufficient evidence of that capability. Preserve the note ID; explicitly acknowledge that note-type/card-template changes can affect card identities and scheduling. A snapshot of fields is not a full scheduling backup. Do not fall back to deleting/recreating the original note if migration is unavailable.

### 9.1 Plan format and review

**REQUIRED new plan envelope:** version, immutable ID/revision, timestamps, collection/profile identity, resolved configuration fingerprint, selector/input records, source note/model/deck/tags/field/media evidence, duplicate decisions, stage results/provenance, canonical documents, destination mappings/model plan, warning/error/review decisions, media manifest, approval revision, and apply receipts. Introduce a plan schema separately from the existing v1 card schema.

Store binary assets durably with content hashes or compatible embedded data; a reviewed plan must remain applicable after cache pruning. A plan references exact document/media revisions. Editing invalidates approval and dependent enrichment as appropriate. Apply never reruns generation silently or substitutes different media behind the preview.

`plans diff` shows actual physical field changes, preserved/cleared/unmapped content, model/template changes, tags, output-note count/splits, media additions/replacements/deletions, and warnings. `plans show --explain` traces source evidence and dependencies. `plans edit` may use `$VISUAL`/`$EDITOR` or import a structured patch; no embedded modal editor is required. Field edits, image class overrides, pronunciation selection, and split/duplicate decisions persist across process exits.

Terminal preview is a readable text/field/media comparison, not a claim of Anki HTML fidelity. Optional `plans export --format html` can produce self-contained review files with local media and no network access. Opening a browser is opt-in and not required for CLI functionality. Arbitrary Anki JavaScript/template rendering is not implemented by merely exporting HTML. **REVIEW R25** confirms preview fidelity expectations.
