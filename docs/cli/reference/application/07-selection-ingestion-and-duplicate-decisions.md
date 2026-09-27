# Application specification reference

## 7. Selection, ingestion, and duplicate decisions

### 7.1 Existing-note selection

Support configured purpose/deck, exact note IDs, Anki query, note type, card template, required/excluded tags, inclusive creation-date range, has/no-image predicate, and maximum result count. Send indexed predicates to Anki. Inspect content predicates in bounded `notesInfo` pages; avoid one unbounded payload for a large deck. Store the resolved IDs, selector, selection time/timezone, and stable order in the plan/job.

Use note IDs to deduplicate results when several card templates/decks select the same note. A note may have cards in more than one deck; avoid treating its first deck as uniquely authoritative. **REVIEW R09** defines destination/deck ownership for these notes.

The Rust selector's `Complete` currently means `-is:new`, and `Incomplete` means `is:new`: that describes review state, not modernization completeness. The new CLI must name these filters accurately or introduce a dedicated modernization predicate. **REVIEW R10**.

Creation dates must document local calendar/Anki-day semantics and inclusive endpoints. Anki `added:N` is relative; preserving only that string is not a reproducible selection later. Save the resolved dates and IDs. Prefer explicit `--from YYYY-MM-DD --to YYYY-MM-DD` plus convenience presets whose resolution is printed.

### 7.2 New input

Inputs: repeated `--word`, UTF-8 line files/stdin, tab-separated expression/context lines, and CSV. CSV compatibility requires `word`, optional `language`, `type`, `note`; explicit column mappings may accept alternative headers. Handle BOM, CRLF, quoted commas/newlines, blank rows, and row/column error locations. Reject missing expression headers unless a mapping is supplied; never silently treat an arbitrary first column as vocabulary.

Normalize HTML/entities and whitespace for exact comparison. Keep original input separately. Existing filters remove ASCII/full-width parentheses and optionally restrict characters; these can erase intentional distinctions, so display changed expressions. Do not introduce lowercase, transliteration, reading-equivalence, or fuzzy duplicate equivalence without review: **REVIEW R11**.

Language aliases currently include Japanese/ja, English/en, Taiwanese/zh-TW, German/de and explicit vocabulary/grammar keys. Clarify that existing `taiwanese` TTS maps to Taiwanese Mandarin, not proof of Taiwanese Hokkien support: **REVIEW R12**.

For each normalized expression, search the mapped destination before expensive enrichment. Candidate search is indexed; final comparison uses normalized expression field text. Results are:

- No exact note: Inject.
- One exact note: Modernize that note.
- Several exact notes: Needs-review with IDs/models/decks; no implicit first match.
- Repeated row within the same input and destination: Skip by default; retain its source line for audit.
- Failed Anki lookup: failed resolution, never proof that the expression is new.

**RECOMMENDED duplicate flags:** `--on-existing modernize|skip|error` and explicit `--resolve-note-id`; introducing duplicates should require a separate deliberate policy. **REVIEW R13** covers allowed duplicate creation and whether valid CSV rows may proceed when others fail. Recommended: prepare all resolvable rows, return partial status, block affected rows only; never silently discard malformed input.

Dictionary/LLM lemma suggestions are proposals. If accepted, redo duplicate resolution and dictionary lookup for the accepted expression. Do not use old-form dictionary evidence while creating a new lemma note. **REVIEW R14** decides whether automatic acceptance is ever allowed; recommended default is explicit acceptance.
