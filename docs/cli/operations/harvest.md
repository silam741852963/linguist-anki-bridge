# Word harvester operations

Read [shared command rules](README.md) before implementing any handler. The harvester (WP-25) finds words met in the deck's own example sentences that the deck has no card for. Candidates live in the store's harvest tables (schema 16) for one purpose, `japanese_vocab` or `english_vocab` (`HARVEST_PURPOSE_UNSUPPORTED` otherwise). No harvest command writes Anki.

## OP-90 — `harvest scan --purpose P [--deck DECK] [--full] [--limit N]`

Inputs: purpose, optional deck (default the purpose's `target_deck`, then its `source_deck`; `HARVEST_DECK_UNSET` when none), `--full`, `--limit N` (changed notes read in this run).

Effects: Anki read only (find and read the deck's notes); writes only the harvest tables.

1. Read every note of the deck. Its word is the `Expression` field, else `Word`, else the first field. Each word counts as known with its head word, its する stem and its hiragana form.
2. Read the latest applied document of every note of the purpose's language from the store. Its forms, readings and dictionary forms count as known; Cambridge `irregular_forms` extend the English lemmatizer. Words marked `ignored`, `known`, `queued` or `added` also count as known.
3. For each applied document, scan its examples, collocations and the sentences of its usage and nuance; for a deck note no apply wrote, scan the visible text of fields whose name contains `example` or `sentence`. A note is read only when its semantic digest changed since its last scan, unless `--full` forgets the marks first.
4. Japanese sentences go through Lindera with the embedded IPADIC dictionary: particles, auxiliaries, symbols, numbers, proper nouns, dependent words and single kana are dropped; a サ変 noun keeps its する; adjacent nouns are joined when the joined form is a deck word or, with `harvest.jmdict_path`, a JMdict entry. English sentences are split into words, lemmatized by rule and the irregular table, and stop words, words with digits and capitalized words inside a sentence are dropped.
5. Each unknown word becomes a candidate (status `new`) with its reading, part of speech, JMdict frequency band and the sentences it was met in, each with its translation, provenance and the other unknown words of that sentence. A `queued` word whose note now exists becomes `added`; a `new` word that became known is forgotten; `new` words no sentence names any more are pruned.

Result/failure: `deck_notes`, `known_words`, `notes_read`, `notes_unchanged`, `from_documents`, `from_fields`, `sentences`, `candidates`, `new_candidates`, `became_added`, `pruned`, `more` (the limit stopped the run) and `jmdict_entries`. `JMDICT_UNREADABLE` when the configured file cannot be read.

## OP-91 — `harvest list --purpose P [--status S] [--min-notes K] [--limit N]`

Inputs: purpose, optional status (default `new` and `queued`), `--min-notes` (default 1), `--limit` (default 50).

Effects: Local read only.

1. Rank the candidates: i+1 sentences (context sentences where the word is the only open unknown word) first, then the number of notes, then the JMdict frequency band, then whether a `dictionary` or `source` sentence attests it.

Result/failure: `total`, `shown` and per candidate the word, reading, part of speech, status, scores and the best sentence with its translation. `HARVEST_STATUS_INVALID` for an unknown status.

## OP-92 — `harvest show --purpose P WORD`

Inputs: purpose and a candidate's base form.

Effects: Local read only.

Result/failure: the candidate's row as in OP-91 and every context: note, sentence, translation, provenance and the other unknown words. `HARVEST_WORD_NOT_FOUND`.

## OP-93 — `harvest ignore --purpose P WORD...`

Inputs: purpose and base forms.

Effects: Local harvest table write.

Result/failure: sets status `ignored` (not worth a card: a name the filter missed, slang). `changed` and `not_found`; exit 1 when a word is not a candidate.

## OP-94 — `harvest known --purpose P WORD...`

Inputs: purpose and base forms.

Effects: Local harvest table write.

Result/failure: sets status `known` (known, no card wanted), as OP-93.

## OP-95 — `harvest unignore --purpose P WORD...`

Inputs: purpose and base forms.

Effects: Local harvest table write.

Result/failure: puts the words back to `new`, as OP-93.

## OP-96 — `harvest queue --purpose P (WORD... | --top N)`

Inputs: purpose and base forms, or `--top N` for the N best ranked `new` candidates.

Effects: Local harvest table write.

Result/failure: sets status `queued`, as OP-93. `HARVEST_TOP_QUEUE_ONLY` when `--top` is given to another status command.

## OP-97 — `harvest export --purpose P [--status S] --output FILE`

Inputs: purpose, status (default `queued`) and a file that must not exist.

Effects: Local file write.

1. Write one v2 vocabulary record per candidate of that status, in rank order: `expression` (the base form), the reading for Japanese, the best context sentence as the first example with provenance `source`, a source summary naming how many notes it was met in, and the tag `lab::harvest`.

Result/failure: `records`, `output` and the `vocab add --document FILE --format jsonl` command (OP-21) that prepares them. `HARVEST_EXPORT_UNWRITABLE` when the file exists or cannot be written. A queued word becomes `added` at the scan after its note exists.
