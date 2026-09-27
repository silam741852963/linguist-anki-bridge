# Batch modernization operations manual

Batch jobs turn the one-card modernization transaction into a durable queue for
hundreds or thousands of Anki notes. They use the same OCR, image
classification, dictionary, Ollama, Kanji, image, TTS, managed-template,
snapshot, and commit code as Preview. Batch mode does not introduce a second
card format.

## Before starting

1. Start Anki Desktop with AnkiConnect enabled.
2. Start Ollama and select the intended model in Status/Settings.
3. Install or refresh the managed Japanese template:

   ```bash
   linguist-anki-bridge --install-japanese-template
   ```

4. Map the deck and first run a few cards through Preview.
5. Leave **Dry Run** enabled for the first batch. Dry-run is copied into the job
   at creation time, so changing the global setting later does not silently
   change an existing job.

## Open and navigate the job screen

Press `b` from the main application. The screen contains a job table, its card
table, and an operational detail panel. Use arrow keys within a table and `Tab`
to switch tables. Press `Escape` to return to the main screen; jobs continue in
the background.

| Key | Operation |
| --- | --- |
| `n` | Open the full job selector; on the Cards table, next page |
| `a` | Create a job from every note in the active mapped deck |
| `r` | Start or resume the selected job |
| `p` | Request a safe pause after the current card boundary; on the Cards table, previous page |
| `f` | Reset exhausted failures so they can be retried |
| `c` | Cancel pending work after confirmation; the current card finishes safely |
| `u` | Revert all committed changes made by the job after confirmation |
| `d` | Delete job state after a keyboard confirmation; never reverts Anki |
| `Left` / `Right`, `PageUp` / `PageDown` | Move through the selected job's bounded card pages |

All confirmations are keyboard-only: press `y` to proceed or `n`/`Escape` to
cancel. There are no mouse-only confirmation controls.

### Build a selection with `n`

The new-job screen combines Anki-native filters with local content predicates:

- mapped deck (also determines field mapping and destination template);
- a date preset (`today`, `yesterday`, this week/month, last three/six
  months, or this year) or an explicit custom From/To range;
- exact note type and card template;
- free text or an advanced Anki query fragment such as `is:due -is:suspended`;
- required and excluded tags;
- cards with images, without images, or either;
- an optional maximum result count.

Use `Tab`/`Shift+Tab` and arrow keys to operate controls. Query and tag inputs
open an IDE-style completion list as soon as they receive focus. An empty query
first offers namespaces such as `is:`, `tag:`, `deck:`, and `note:`; accepting
a namespace immediately offers its valid next values. Press Down to enter the
completion list and Enter to accept an item. `c` counts the exact selection,
`s` schedules it, and `Escape` cancels. Deck, age, note type, card
template, tag, and text predicates are sent to AnkiConnect so Anki's indexed
search does the heavy filtering. Presets become an exact inclusive combination
of Anki's `added:N` and `-added:N` operators. Only image predicates require
bounded `notesInfo` batches in the app. The immutable selector is stored in job settings
for auditing and reproducibility.

Only one batch executes at a time. This deliberately keeps Anki writes ordered,
reduces load on local Ollama, and makes external service pacing predictable.
An OS-level runner lease also prevents a second application process from
executing or rolling back jobs. The kernel releases the lease automatically on
normal exit or a crash; another process may still inspect the durable tables.

## Job lifecycle

`queued` jobs have not started. `running` jobs are active. `pausing` means the
current card is allowed to finish; it then becomes `paused`. A job is
`completed` only when every card completed. It becomes `failed` when at least
one card exhausts its retry budget. Use `f`, then `r`, after correcting the
cause. In a `cancelled` job, cards that never started become `skipped`.

Each card advances through durable states:

1. `pending` — eligible for processing after any retry delay.
2. `processing` — external enrichment is running.
3. `processed` — its complete result is stored as an atomic JSON artifact.
4. `committing` — the original note/media snapshot exists and the Anki update
   is being applied.
5. `completed`, `failed`, `skipped`, or `reverted` — terminal/operator states.

The screen polls in a background worker every 1.5 seconds, rebuilds tables only
when the durable job revision changes, and displays card rows in pages of 200.
The Cards pane title shows the current page and total (for example, `2/5`).
Individual errors and attempt counts remain visible without forcing Textual to
construct tens of thousands of widgets every second.

## Rate limits and retries

The `batch` section in `config.yaml` controls scheduling:

```yaml
batch:
  max_attempts: 3
  retry_backoff_seconds: 5.0
  commit_interval_seconds: 0.25
  service_intervals:
    dictionary: 1.0
    ollama: 0.25
    kanji: 1.0
    image: 1.0
    tts: 0.5
```

Intervals are minimum seconds between starts for the named service. Each
service owns an independent limiter, so waiting for Jisho does not incorrectly
consume the Ollama allowance. Failures use exponential delays: with the
defaults, retries wait about 5 and then 10 seconds. A commit retry reuses the
already persisted OCR/dictionary/LLM/media artifact instead of repeating costly
external work.

Increase `dictionary`, `kanji`, `image`, or `tts` intervals when a provider
returns 429/503 responses. Increase `ollama` when local GPU memory pressure or
model queueing is high. Do not set aggressive values merely because a provider
occasionally responds quickly; sustained rates matter.

## Abrupt shutdown and recovery

SQLite runs in WAL mode with full synchronous durability. Large base64 results
are outside SQLite and written to a temporary file before an atomic rename.
This prevents a half-written result from being treated as complete.

On the next application start:

- a formerly running job becomes `paused` rather than starting unexpectedly;
- `processing` cards return to `pending` because enrichment is read-only;
- `committing` cards return to `processed` and retain their pre-write snapshot.

Resume with `r`. The managed-field and media write is idempotently replayed if
shutdown happened after Anki accepted the write but before the local completion
checkpoint. The original snapshot is reused, so recovery never replaces it
with a snapshot of the already-modernized card.

## Mass rollback

Select the job and press `u`. Rollback restores completed cards in reverse
order using the exact snapshots captured immediately before their commits.
Fields, original note type, tags, and tracked media are restored by the existing
SnapshotManager. Re-running rollback is safe: reverted snapshots are recognized
as already restored.

Rollback refuses a card when a newer completed batch changed the same Anki note.
This conflict check prevents an old job from erasing newer work. Such cards are
marked `rollback_failed`; review their word-level snapshots in Preview and
resolve them deliberately. Other cards continue rolling back, and the job ends
as `rollback_partial` instead of pretending the operation fully succeeded.

Do not delete the batch database or snapshot file before a rollback. They are
the recovery journal.

### Delete a job

Press `d`, read the warning, then press `y`. Deletion removes the job/item rows
and cached processing artifacts only. It **does not revert any committed Anki
notes**. Run `u` and verify rollback first if undo may be needed. Word-level
snapshot records remain in the snapshot store, but deleting the job removes the
mass-rollback association and its audit trail.

## Storage and backup

Persistent state is under `~/.config/linguist-anki-bridge`:

- `batch_jobs.sqlite3` — job/item state and checkpoints;
- `batch_jobs.sqlite3-wal` / `-shm` — live SQLite WAL files;
- `batch_jobs_artifacts/<job-id>/<item-id>.json` — processed card artifacts;
- `card_snapshots.json` — pre-write restoration records.

Back up this directory together with the Anki collection when long-running jobs
are operationally important. Artifact files may be large because they preserve
media needed for commit replay. Automatic pruning is intentionally not done in
0.0.1: deleting recovery evidence is an operator retention-policy decision.

## Design rationale

- **SQLite instead of a single queue JSON:** atomic transactions, indexed
  inspection, WAL crash recovery, and scalable status updates without rewriting
  every job.
- **Artifacts outside SQLite:** keeps queries responsive and avoids amplifying
  writes of base64 images/audio; atomic rename still provides a hard checkpoint.
- **Card-boundary pause/cancel:** avoids interrupting Anki model/media
  transactions in an unknown state.
- **One active worker:** stable Anki ordering and conservative third-party load
  are more valuable than unconstrained throughput. Independent service pacing
  still removes unnecessary sleeps.
- **Kernel-released process lease:** prevents duplicate workers without a stale
  PID/lock cleanup problem after abrupt termination.
- **Exponential bounded retry:** transient 502/429/network errors recover, while
  permanent schema/model errors stop after a visible finite number of attempts.
- **Snapshot before commit:** every mutation has compensating data before the
  first write. Snapshot identity is stored and reused across crash recovery.
- **Reverse rollback plus newer-write guard:** mirrors transaction unwinding and
  prevents lost updates across overlapping jobs.
- **Immutable job settings:** a running/resumed job behaves the same after a
  restart even if global configuration is edited later. Operational pacing,
  retries, and dry-run are captured for auditability.
- **Server-side selection plus bounded metadata batches:** Anki's indexes handle
  deck/time/type/card/tag/text predicates; local filters never require one huge
  AnkiConnect response.
- **Change-aware paged monitoring:** SQLite polling happens off the UI loop and
  row reconstruction is proportional to one visible page, not total job size.
