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

- [x] **Deleted-note reporting in `retag.rs`, with a test.** Done in 1a30dc4.
  - 66 notes show as `RETAG_NOT_APPLIED: the latest operation … is restored`.
  - An apply created each of them and a snapshot restore deleted it again;
    `notes show` gives `ANKI_NOTE_NOT_FOUND`.
  - Fix: leave such notes out, or report them as a distinct
    `RETAG_NOTE_DELETED`. Add a CHANGELOG entry.
- [x] **Fix-up campaign for the 6 real notes** (ran 2026-10-25; all deferred, see Progress), built the same way as
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
- 2026-10-25 20:21: **step 1 done.** Every applied note that can be
  retagged now has the current render:
  - 3,024 notes in the final run under checkpoint c13e7836, 0 pending;
    4,087 change-or-current in total.
  - Still skipped: 66 `RETAG_NOTE_MISSING` (deleted by restores), 1
    `RETAG_NOT_APPLIED` (歩留まり) and 5 `RETAG_RENDER_BLOCKED`. These 6
    real notes are the fix-up campaign item above.
  - Per-note write time went from 8.7 s to 0.53 s: companion 0.6.1
    (93e2089), checkpoint hashed once, read retries (c894564), fewer round
    trips and unpooled AnkiConnect connections (7fc5d91).
  - The dropped connections that stopped earlier runs came from reqwest
    reusing connections that AnkiConnect closes silently.
  - Two stops were settled with `recover reconcile`.
- 2026-10-25 20:25: fix-up campaign `f8cf60c2-f783-48ad-8a24-c94ffe5d140f`
  created from `target/lab/wp24/spec-fixup-retag.json` (the 6 notes above, one
  batch). A session cron one-shot at 20:31 runs it first, then starts the
  main campaign 42e5a4e4.
- 2026-10-25 20:35: fix-up f8cf60c2 finished and wrapped up, with nothing
  written; all 6 notes are deferred:
  - 内面, 外面, 門, 粉, 的: `AGENT: multi-word note, split outside the
    campaign`. Each Pronunciation holds two readings that are different
    words (ないめん/うちづら, がいめん/そとづら, もん/かど, こな/こ,
    まと/てき); the draft had picked the secondary reading for 粉 and 的.
    They need `plans split-vocab` outside a campaign (open item).
  - 歩留まり: `PREPARE_FAILED: SOURCE_CAPTURE_REVIEW_REQUIRED
    (SOURCE_MAPPING_MODEL_CONFLICT)`. The spec maps Linguist Vocabulary v3
    fields, and the note is v2. It needs a source with a v2 mapping (open
    item).
- 2026-10-25 20:35: main campaign 42e5a4e4 resumed through
  `campaign-session.sh start` (driver pid 3555466); agent turns run from the
  session watch.
- 2026-10-25 20:35-21:10, first agent turn on 42e5a4e4:
  - Approved batches 273 (after `plans bind --renew`, rev 22), 281 and 282
    after spot-checks.
  - Split the grammar pages of 283 (5 notes into 10 units: とは ×3,
    だに, なり ×3, つ〜つ, といったところだ ×2), 284 (5 notes, 1 unit each)
    and 285 (5 notes, 1 unit each), with corrected OCR text and English
    translations. The next ticks re-review them.
  - Batches 274-280 have no meaning drafts yet (`REQUIRED_CONTENT`) and wait
    for Codex, which cools down until 01:32. 280 also logged a
    `REVISION_CONFLICT`; check it after the next tick.
- 2026-10-26 17:42: session started (driver pid 94185). Agent turn:
  - 283-285 splits went through; their units now wait for Codex meaning
    drafts (`REQUIRED_CONTENT`), like 274-280. 280's `REVISION_CONFLICT`
    cleared by itself.
  - Batches 287 and 289 hold **keigo drill cards**, not grammar pages: deck
    夕暮れの詞, note type "Cơ bản", front a screenshot of a plain sentence,
    back a screenshot of its polite business forms. The user wants them
    **kept as drills, but as text instead of images**. No managed note type
    can hold that, so the 10 notes are deferred (`AGENT: keigo drill card
    …`). Open item: a text drill note type and a campaign path that turns
    the two screenshots into prompt and answers. Until then, defer every
    drill card the campaign meets. 夕暮れの詞 has 317 image-only "Cơ bản"
    notes; some are grammar pages, the rest drills.
- 2026-10-26: the user chose to keep the keigo drills as drills, in text,
  answered by tap-build on the phone. Plan: [WP-28](wp-28.md). Until it
  ships, agent turns defer every drill note.
- 2026-10-26 17:53: batches 287 and 289 closed with every note deferred;
  290 and 291 are drills too (10 notes deferred; 290 repeats 289's five
  drills word for word, so the deck holds duplicate drills, a WP-28 merge
  case). 20 drill notes deferred in all. Codex is drafting 274-285.
- 2026-10-26 evening, agent turns:
  - 292-294 drill notes deferred (35 drill notes deferred in all, batches
    287 and 289-294). 295 and 297-300 split with authored units (OCR fixed,
    English translations); 298 なりに split in three uses, だろうに and
    あれで in two.
  - 282, 283, 284 and 285 approved after spot checks.
  - Codex drafts time out often (100-240 s per call). Deferred with
    `GENERATION_FAILED`: 284 にもまして, three 285 units and four 297 units.
    Their authored units are kept for a re-split. The fix (settling a note
    retries its drafts, commit 34f6878) is not in the lab binary yet.
- 2026-10-26: WP-28 implemented (commit 4deb84f), not yet live. Rollout
  while the campaign is paused: lab binary, companion 0.7.0, `models
  install japanese_drill --apply`, then the deferred drills return at the
  next `campaign-session.sh start` and agent turns convert them with
  `drills.py convert`. Steps in [WP-28](wp-28.md#progress).
