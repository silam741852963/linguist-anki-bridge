# Research reference

## 6. Delivery sequence and testable completion

### Milestone A — collection-safe CLI skeleton and integration spike

Add `linguist-cli` without Qt/Textual dependencies; clap/help/JSON/errors; config import; read-only doctor/inventory. In a disposable Anki profile, prove supported native migration/backup/identity integration, explicit vocab 0→0/1→1/2→2 and grammar 0→0 mapping, undo and after-state reading. Record actual installed Anki version (local package is `anki-git`, so release-name assumptions are insufficient). Do not enable real migration until this gate passes.

Deliverables: CLI startup/subprocess tests, generated command reference, verified action matrix, backup restore test, compatibility extension design or documented native fallback.

### Milestone B — standard documents/models and four preparation paths

Implement typed vocabulary/grammar v2, versioned schemas, source mapping, article/image/text/CSV inputs, rich dictionary preservation, grammar OCR/layout/unit segmentation, duplicates, persistent plans/assets, field/media diffs and external edits. Move live adapters out of desktop backend. Correct source evidence ordering before generation. Add template assets under shared Rust/application resources; Python compatibility can package copies as needed.

Deliverables: one prepared plan from each of the four workflows; source-preservation fixtures; screenshot-only/multi-pattern fixtures; user edits surviving restart; migrated v1 contract fixtures.

### Milestone C — useful vocabulary and grammar enrichment

Connect installed model capabilities, schema generation/validation, pronunciation preservation, optional internet images and Japanese Kanji. Benchmark Tesseract/Gemma and optional PaddleOCR on representative grammar/vocab screenshot regions. Validate explanation/translation quality with review, not schema alone. Use new English grammar author inputs to prove the path even though the inspected collection has no identified English grammar deck.

Deliverables: source-linked extracted grammar, checked negatives/formation, usable contextual exercises, correct pronunciation associations, no fabricated dictionary senses. Record failures/uncertainties as review items.

### Milestone D — exact-plan apply and recovery

Implement one-note additions, mapped migrations, operation markers, before/after conflict guards, snapshots/media journal, backup checkpoint, read-back verification and conflict-aware restore. Then implement reviewed source expansion/splits. Fault-inject between all mutation steps, including API acceptance before receipt persistence, sibling creation and partial restoration. Existing history stays unchanged for retained tasks; newly generated tasks are new cards.

Deliverables: four workflows apply and restore successfully in a disposable collection; schedule/card identity evidence for old vocab/grammar; no blind duplicate creation on retry; no shared-original-media deletion.

### Milestone E — durable batches and release

Reuse SQLite jobs for both kinds and both modes. Freeze configuration/plan revision, lease writer, preserve artifacts, pace providers, pause/resume safely, list bounded pages, rollback reverse commit order and retain conflicts. Back up active SQLite stores using a consistent procedure; SQLite's [online backup API](https://www.sqlite.org/backup.html) provides snapshot copying for live databases. Package one core binary plus optional extras; publish compatibility/upgrade notes and shell completions.

Deliverables: crash/restart/retry/rollback evidence; full four-workflow command examples; installable CLI; tested legacy-store migration; explicit deferred optional providers.
