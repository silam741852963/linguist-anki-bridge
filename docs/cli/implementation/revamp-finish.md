# Finishing the collection revamp

Plan agreed on 2026-10-25. It covers the steps after WP-27: from where the
live campaign stands, through to the final AnkiWeb upload. Every session
works from this file. Update the **Progress** section at the end of each
session.

Main campaign: `42e5a4e4-d094-47b0-96ff-2625f3ec36dd` ("Collection revamp
2026-10"). Run commands with the lab binary `target/lab/linguist-anki-bridge-head`
and `ANKICONNECT_API_KEY` from `~/.config/linguist-anki-bridge/ankiconnect-api-key`.

## Starting point (2026-10-25 17:30 JST)

- 3,974 notes applied, 14,834 not started, 105 grammar notes waiting for
  the agent (batches 273-284). Campaign paused ("wrapped up"). The Codex
  window resets at 20:29 JST.
- Notes drafted from now on get their profile (domain, register, connotation)
  in the same enrichment call, and the new CSS colours when they are applied.
  They need no extra step.
- About 3,950 notes were applied before WP-27. They still have the old look,
  apart from 40 that were retagged for review: 俳優 … 腰, then アパート … クラブ
  in plan 5f010626.

## Step 1: retag the applied notes (one run, no Codex)

Run this while the campaign is paused, because retag needs the writer lease.

```sh
linguist-anki-bridge-head --output json notes retag --all            # preview
linguist-anki-bridge-head --output json notes retag --all --apply    # one checkpoint
```

- It re-renders every applied note from its stored document. It adds the
  colours, `lab::pos::*` and `lab::connotation::*`, plus `lab::domain::*`
  where a profile already exists.
- It skips a note the user edited after its last apply. It also skips a note
  it cannot render; the preview showed 5 such notes, all `AUDIO_NOT_FOUND`.
- Read a few notes back with `notes show` afterwards. Record the checkpoint
  id under **Progress**.

### Step 1 follow-ups (from the 2026-10-25 preview: 4,048 change, 39 current, 72 skipped)

- [ ] **Deleted-note reporting in `retag.rs`, with a test.**
  - 66 notes show as `RETAG_NOT_APPLIED: the latest operation … is restored`.
  - An apply created each of them and a snapshot restore deleted it again;
    `notes show` gives `ANKI_NOTE_NOT_FOUND`.
  - Fix: leave such notes out, or report them as a distinct
    `RETAG_NOTE_DELETED`. Add a CHANGELOG entry.
- [ ] **Fix-up campaign for the 6 real notes**, built the same way as
  `target/lab/wp24/spec-fixup.json`. It costs about 6 Codex draft calls, so
  run it after a Codex reset.
  - 歩留まり 1791366792727: an old `Linguist Vocabulary v2` note whose apply
    was restored.
  - 内面 1654787651706, 外面 1654787707997, 門 1658496172086,
    粉 1668253661721, 的 1671805775824: `RETAG_RENDER_BLOCKED`
    (`VOCAB_SPLIT_REQUIRED`, `AUDIO_NOT_FOUND`, `GENERATION_ENGINE_*`).
    They were applied before those rules existed and need a redraft and a
    split.
- Gotcha: a retag `--apply` reads every note first (about 5.5 min) and fails
  with `ANKI_DEPENDENCY_UNAVAILABLE` if other AnkiConnect calls run at the
  same time. Run nothing else against Anki during a retag.

## Step 2: finish the campaign (many sessions)

Each session runs the same loop:

1. `scripts/campaign-session.sh start 42e5a4e4-d094-47b0-96ff-2625f3ec36dd`.
   It requeues deferred notes, resumes the campaign and starts VOICEVOX. It
   never starts Anki: the user starts Anki.
2. Agent turns run from the session through a background
   `campaign-session.sh watch`. The `campaign-tick` skill does the work:
   spot-check and approve ready batches, settle or defer items, and convert
   grammar pages with `target/lab/wp24/grammar.py`.
3. The driver wraps up by itself when Codex passes
   `llm.agents.codex.max_window_percent` (65). Then:
   - run `target/lab/wp24/wait-wrap.sh 42e5a4e4-...` in background Bash; it
     prints `RESET <epoch> <time>`;
   - book a one-shot CronCreate (recurring false) at the reset minute + 1
     that runs step 1 of this loop.
   Never install timers.
4. The user ends a session with `scripts/campaign-session.sh wrap-up`.
5. Every finding goes into code, with a CHANGELOG entry per fix.

Throughput so far is about 100-150 Codex draft calls per 5-hour window at
the 65% cap. The remaining ~14,900 notes therefore need roughly 100+ windows.
Agent work (grammar pages, image roles, sense choices) is the other limit.

## Step 3: domain backfill for the old notes (after the campaign)

`plans classify` judges the profile of applied documents that have none. It
sends 20 notes per Codex call and writes one new local revision per plan; it
makes no Anki writes. Per plan:

```sh
linguist-anki-bridge-head --output json plans classify PLAN --base-revision N --digest D --limit 20 --run
linguist-anki-bridge-head plans approve PLAN ...      # as claude-agent, after spot-checks
```

Then run one `notes retag --all --apply`, which writes the domains as tags.

Never approve these dead revisions:
- 517c483c rev 34 and rev 35 (rev 35 is approved, but the policy replay
  rewrote its examples);
- f9e10a25 rev 18 and rev 19.
Reclassify those plans from a fresh base instead.

### Cost estimate

- Calls: about 3,950 notes / 20 = **about 200 Codex calls** (gpt-6.1-sol, low
  reasoning).
- Each call took 13-20 s in the live runs (15:29, 15:38, 15:47 on
  2026-10-25), against 6-15 s for a single-note draft call. Count a classify
  call as about 1.5-2 draft calls of quota, so about 300-400 draft-call
  equivalents.
- That is about **3-4 five-hour windows** at the 65% cap, or about 2-3% of
  the Codex cost of the remaining campaign.
- These are estimates. `scripts/codex-meter.sh` now logs `tokens: null`
  because the Codex CLI changed its "tokens used" output. Measure the real
  cost from the window percentage before and after the first 10 calls.

## Step 4: close and upload

1. `campaign close 42e5a4e4-...` once every batch is done or skipped.
2. The user syncs in Anki and chooses **Upload** (full sync). Until then,
   review only in this desktop profile.
3. Close the fix-up campaigns (a947104e, 3742ef1a) the same way.

## Resume prompt for a new session

> Resume the revamp plan in docs/cli/implementation/revamp-finish.md: read
> its Progress section, do the next open item, and update Progress when you
> stop. Anki is open.

## Progress

- 2026-10-25: plan written. Campaign paused until the 20:29 reset.
- 2026-10-25 18:00-18:45, step 1 partly done:
  - 155 notes retagged: 72 under checkpoint 027af94c, 83 under the next
    run's checkpoint.
  - Stopped by the user at 8.7 s per note, which would have taken ~10 h for
    the rest. The stop was clean: 0 pending operations.
  - Fixes: 1a30dc4 (retried reads, deleted notes reported as missing),
    4d68ed9 (lease renewed per note), 93e2089 (companion 0.6.1 ledger scan,
    checkpoint hashed once).
  - **Before the next run:** the user installs
    `target/lab/companion-0.6.1/linguist_bridge-0.6.1.ankiaddon` in Anki
    (Tools > Add-ons > Install from file) and restarts Anki. Then run
    `notes retag --all --apply`, only on the user's instruction. The 155
    notes already done show as current.
