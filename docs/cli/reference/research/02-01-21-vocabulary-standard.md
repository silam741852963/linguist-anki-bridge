# Research reference

### 2.1 Vocabulary standard

One unit has an accepted expression, reading/pronunciation, selected learning sense or closely related sense cluster, concise answer, usage note, complete examples, original/personal context, optional visual mnemonic, pronunciation tracks, and Japanese Kanji details where applicable. Preserve all dictionary entries/senses in reference data, but do not make recalling a complete dictionary entry the grading task.

**Target regular note type:** `Linguist Vocabulary v2`, reusable across target languages, with fixed field order:

| Field | Purpose |
| --- | --- |
| Expression | Accepted headword/phrase; no automatic silent lemmatization. |
| Reading | Japanese reading or other language-specific orthographic reading. |
| Pronunciation | IPA/accent/locale labels, separate from recording filenames. |
| Meaning | Concise selected recall answer followed by clearly separated source reference. |
| Usage | Register, restrictions and useful nuances, labeled by provenance. |
| Examples | Sentence/translation pairs; preserve complete relevant OCR examples. |
| Picture | Approved visual mnemonic; optional, never an arbitrary required stock image. |
| Audio | One or more aligned original/dictionary/TTS tracks. |
| Kanji | Japanese-only supporting details; empty for other languages. |
| PersonalNotes | Preserved author context and legacy personal material. |
| Source | Human-readable source/attribution links and source summary. |
| Language | Target-language tag used for layout/language attributes. |
| SenseKey | Stable normalized identity for the reviewed sense/learning unit. |
| EnableProduction | Optional generated production task. |
| EnableSpelling | Optional generated spelling task. |
| ProductionPrompt | Reviewed disambiguated production cue. |
| SpellingPrompt | Reviewed audio/context cue without answer leakage. |
| ExplanationLanguage | Explanation/translation BCP-47 language. |

Three templates in order: **Comprehension (0), Production (1), Spelling (2)**. New notes default to Comprehension; opt in to other tasks when their prompts are useful. Revamped notes preserve existing tasks/card mappings and existing spelling intent; do not silently discard mature Production/Spelling cards merely because new-note defaults differ.

Comprehension: expression → concise meaning; reading/audio/examples/reference after reveal. Production: reviewed picture or tightly specified sense/context → expression, with enough disambiguation to avoid several equally valid answers. Spelling: audio/appropriate cue → written expression, using native typed-answer comparison where applicable. Do not put the target spelling in its own prompt. Alternative valid answers are displayed; native typed-answer comparison is assistance, not a semantic grader.

Conditional front templates require their prompt/answer prerequisites. Anki's [card-generation rules](https://docs.ankiweb.net/templates/generation.html) support field-based gating; emptying an existing front does not immediately delete the old card. Therefore preserving an existing card whose new task lacks a valid prompt requires a reviewed retained/legacy cue or a separately approved suspension/removal policy. No automatic Empty Cards cleanup.
