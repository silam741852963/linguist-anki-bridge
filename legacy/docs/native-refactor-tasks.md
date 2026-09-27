# Native refactor task queue

Work through this file in order. Each task is sized for one focused agent turn
and one commit. Do not begin a later task until the prerequisite tasks pass.

## Rules for every task

1. Read `docs/native-refactor.md` and the files named by the task.
2. Preserve the contract and recovery invariants in `linguist-core`.
3. Keep provider and persistence code out of QML.
4. Keep Anki mutations behind snapshots, validation, and rollback.
5. Add focused tests for new domain, parsing, or state-transition behavior.
6. Run `cargo fmt --all -- --check`, the affected Cargo tests, and
   `PYTHONPATH=src .venv/bin/python -m pytest -q` when Python changes.
7. End with a short handoff: changed files, checks run, and the next task ID.

## Phase 1 — migration contract

### N01 — Add Python golden-document exporter

**Prerequisites:** foundation commit.

Add a development command that processes supplied fixture data through the
existing Python `build_card_document`, exports contract v1 JSON, and writes only
to an explicitly supplied output path. Add fixtures for modernization,
injection, shared field mappings, media replacement, and validation issues.

**Files:** `src/linguist_anki_bridge/contracts.py`, a new development script,
`contracts/fixtures`, and contract tests.

**Done when:** fixtures contain no network-dependent values, round-trip through
Python, and deserialize in `linguist-core` tests.

### N02 — Complete card-contract parity

**Prerequisites:** N01.

Expand Rust card mapping tests to consume every golden fixture. Reproduce the
Python mode policies for modernization and injection, including tags, context,
media retention, obsolete media, audio rows, and readiness issues.

**Files:** `crates/linguist-core/src/card.rs` and fixture tests only.

**Done when:** every deterministic card outcome has an explicit Python/Rust
parity assertion. Do not port network enrichment in this task.

### N03 — Version batch and snapshot contracts

**Prerequisites:** N01.

Define JSON Schemas, Rust types, and Python import/export adapters for batch
settings, processed artifact references, and snapshots. Preserve unknown
historical source fields in a dedicated extension map so migration is lossless.

**Done when:** existing Python snapshot examples and batch settings round-trip
without losing note IDs, fields, tags, model/deck data, or media.

## Phase 2 — read-only Anki path

### N04 — Implement the AnkiConnect transport

**Prerequisites:** N02.

Create `crates/linguist-anki` with an async HTTP transport, request/response
envelopes, configurable URL and timeout, and structured errors. Implement only
`version` and `requestPermission` initially. Use a local mock HTTP server in
tests; tests must not require Anki.

**Done when:** transport, AnkiConnect error, timeout, and malformed-response
tests pass.

### N05 — Add read-only deck and note queries

**Prerequisites:** N04.

Implement deck names, model names/fields/templates, note lookup, note info, and
media retrieval. Convert raw Anki payloads into application types at the adapter
boundary.

**Done when:** recorded AnkiConnect responses cover empty decks, missing notes,
HTML fields, tags, and multiple note models.

### N06 — Implement exact-expression resolution

**Prerequisites:** N05.

Port exact existing-note lookup and the decision between modernization and
injection. Keep the decision as a core/application use case so CSV, manual
input, and future integrations share it.

**Done when:** whitespace, HTML, duplicate, no-match, and ambiguous-match cases
are tested without launching Qt.

## Phase 3 — safe write path

### N07 — Add the native snapshot repository

**Prerequisites:** N03.

Implement atomic snapshot persistence below the existing XDG configuration
directory. Read existing Python snapshots and write the versioned representation
without deleting or rewriting historical records.

**Done when:** interrupted writes, corrupt trailing data, injected-note
snapshots, and media snapshots have recovery tests.

### N08 — Add media transaction primitives

**Prerequisites:** N05, N07.

Implement media lookup, conflict detection, staging, content verification,
rollback, and obsolete-media removal. Model the operation as a transaction whose
rollback data exists before its first mutation.

**Done when:** failures at every mutation boundary restore the mock Anki media
state exactly.

### N09 — Port managed-template validation

**Prerequisites:** N05.

Move the Japanese vocabulary model specification into a language-neutral
fixture or generated Rust module. Implement schema comparison and safe template
refresh planning without performing writes.

**Done when:** legacy one-card, current three-card, foreign, reordered-field,
and changed-template cases produce explicit plans or errors.

### N10 — Implement backup and commit orchestration

**Prerequisites:** N02, N07, N08, N09.

Implement the full write order: validate, back up the affected deck, capture a
snapshot, stage media, migrate/install the managed model if required, update or
create the note, finalize the snapshot, then remove obsolete media. Roll back
on failure.

**Done when:** modernization and injection pass failure-injection tests at each
step. Dry-run must perform no external mutation.

### N11 — Implement snapshot restore

**Prerequisites:** N10.

Restore fields, tags, deck/model identity, and media; remove notes created by
injection. Make repeated restoration safe and report newer-write conflicts.

**Done when:** Python-created snapshots can restore a mock collection and a
second restore changes nothing.

## Phase 4 — durable jobs

### N12 — Create the SQLite job repository

**Prerequisites:** N03.

Create `crates/linguist-jobs`. Reproduce the existing schema semantics using WAL,
foreign keys, full synchronization, bounded page queries, immutable settings,
and atomic artifact replacement. Add migration metadata; do not alter an
existing database yet.

**Done when:** repository tests cover creation, duplicate note IDs, paging,
counts, and atomic artifacts.

### N13 — Port claim and recovery transitions

**Prerequisites:** N12.

Use `linguist-core` states for atomic claim, pause, cancellation, retry delay,
interruption recovery, and completion calculation. Preserve commit-artifact
reuse and the single-runner lease.

**Done when:** state-machine and two-process lease tests match current Python
behavior.

### N14 — Port rollback and retention operations

**Prerequisites:** N11, N13.

Implement reverse-order batch rollback, newer-change conflict checks, partial
rollback status, retry of rollback failures, and deletion of job artifacts
without deletion of snapshots.

**Done when:** overlapping jobs cannot erase newer changes and deletion tests
prove Anki is never called.

### N15 — Add a Python-database compatibility audit

**Prerequisites:** N12–N14.

Open a copied Python batch database read-only, validate every row/artifact, and
produce a migration report. Then implement an explicit backup-first migration
command. Never migrate automatically during GUI startup.

**Done when:** migration of queued, paused, failed, completed, and interrupted
fixtures is tested and the original fixture remains byte-identical.

## Phase 5 — real desktop state

### N16 — Add the application state controller

**Prerequisites:** N05, N06.

Replace QML sample service labels with a Rust controller that reports Anki and
Ollama availability, active deck, selection, busy state, and user-facing errors.
Expose commands and immutable view state through CXX-Qt.

**Done when:** QML has no transport logic and controller tests use fake ports.

### N17 — Bind decks and the review queue

**Prerequisites:** N16.

Replace sample deck/card arrays with Rust list models. Support loading, empty,
error, ready, needs-review, and failed states; selection must survive model
refresh when the note still exists.

**Done when:** keyboard and pointer selection drive the same controller command,
and a 50,000-row model does not instantiate every delegate.

### N18 — Implement editable review drafts

**Prerequisites:** N02, N17.

Bind expression, meanings, Kanji, images, audio, issues, and provenance. Support
normal undoable text editing, accepting/rejecting individual generated changes,
field locks, and regeneration requests that preserve user-edited fields.

**Done when:** switching cards cannot lose a dirty draft and destructive
navigation prompts only when autosave fails.

### N19 — Connect commit, dry-run, and restore

**Prerequisites:** N10, N11, N18.

Connect preview/apply/restore commands. Show the planned field/media/model
changes before a write, retain dry-run as the default, and display the resulting
snapshot in history.

**Done when:** the GUI can complete one Japanese modernization and one injection
against a mock adapter, including restoration.

### N20 — Add rendered card preview

**Prerequisites:** N09, N18.

Render comprehension, spelling, and production front/back views with the real
managed HTML/CSS. Add audio playback, image zoom, narrow-card sizing, and a safe
local-media URL scheme.

**Done when:** preview fixtures render without network access and unsafe remote
navigation is blocked.

### N21 — Connect batch management

**Prerequisites:** N14, N17.

Add paged jobs/items models and commands for create, pause, resume, retry,
cancel, rollback, and delete. Keep confirmation dialogs pointer- and
keyboard-accessible.

**Done when:** a job continues after closing its screen, interruption reopens it
paused, and the UI never loads all item rows at once.

## Phase 6 — enrichment adapters

Implement these independently. Each adapter must expose typed output, timeout,
cancellation, retry classification, fixture-based parsing tests, and cache keys.

### N22 — Ollama discovery and structured generation

**Prerequisites:** N04. Port model discovery, JSON normalization, vocabulary
annotations, grammar generation, and uncertain-image adjudication.

### N23 — Tesseract OCR and preprocessing

**Prerequisites:** N02. Invoke Tesseract as a cancellable child process and port
language selection, preprocessing, OCR evidence, and classification inputs.

### N24 — Japanese dictionary adapter

**Prerequisites:** N04. Port Jisho API/HTML parsing, retry classification, exact
and related entry ordering, and browser-fallback interface. Keep browser choice
behind a port.

### N25 — Other dictionary adapters

**Prerequisites:** N24. Port Cambridge, Moedict, dict.cc, and custom schema
extraction one provider per commit.

### N26 — Kanji adapters

**Prerequisites:** N04. Port primary lookup, character-level fallbacks, stroke
order media, and structured summaries.

### N27 — Image discovery and normalization

**Prerequisites:** N08. Port candidate search, validation, information checks,
RGB JPEG normalization, classification, and visual-recall retention.

### N28 — Audio discovery and TTS

**Prerequisites:** N08. Port dictionary audio, local voice selection, and remote
TTS fallback. Retain multiple pronunciation rows and deterministic filenames.

### N29 — Assemble the enrichment pipeline

**Prerequisites:** N22–N28.

Compose adapters through application ports with bounded concurrency, per-service
rate limits, cancellation, progress events, caching, and partial failure issues.

**Done when:** all golden inputs produce parity-equivalent drafts and commit
retries reuse processed artifacts without repeating enrichment.

## Phase 7 — input, settings, and release

### N30 — Port manual and pasted-list ingestion

**Prerequisites:** N06, N17. Support multiple expressions, contextual notes,
language/type selection, duplicate resolution, and enqueue preview.

### N31 — Port CSV ingestion

**Prerequisites:** N30. Add drag/drop, column mapping preview, row validation,
aliases, and per-row duplicate decisions.

### N32 — Port batch selectors

**Prerequisites:** N05, N21. Add deck/date/model/template/query/tag/image filters,
completion, count preview, immutable selector storage, and bounded metadata
fetches.

### N33 — Migrate configuration and watch Omarchy themes

**Prerequisites:** N16. Read existing YAML without rewriting it, introduce a
versioned native config, import field/deck/provider settings, and watch the
active Omarchy palette path for changes. Test missing and malformed themes.

### N34 — Accessibility and keyboard pass

**Prerequisites:** N17–N21, N30–N33. Define tab order, accessible names, focus
indicators, shortcuts, Japanese IME behavior, scaling, reduced motion, and
screen-reader semantics. Vim-style modal editing may be added as opt-in only.

### N35 — Packaging and parallel-install release

**Prerequisites:** N01–N34.

Package the native binary and QML assets for Arch/Omarchy under a distinct
executable name. Keep Python available during the preview release. Document
config/snapshot/database backups and rollback to the Python application.

**Done when:** a clean package install can modernize, inject, batch, recover
after forced termination, and restore cards using migrated data.
