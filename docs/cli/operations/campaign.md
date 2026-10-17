# Campaign operations

Read [shared command rules](README.md) and [ALG-CAMPAIGN](../recovery/jobs.md#alg-campaign-collection-revamp-campaign) first. A campaign (WP-24) revamps a frozen selection of existing notes batch by batch, unattended, through the existing crash-safe commands. Its state lives only in the local store; a tick, an agent run or a user command can stop at any point and the next one continues.

## OP-62 — `campaign create --spec FILE [--dry-run]`

Inputs: a JSON spec (`contracts/v2/campaign-spec.schema.json`): sources (Anki search, purpose, per-source `--set` overrides, target note types, the fields the cohort rules read, language, duplicate merge), cohort order (`SOURCE:CLASS`, classes `duplicate`, `split_candidate`, `kana_only`, `missing_media`, `plain`), batch sizes, an optional pilot, explicit `merge_extra` groups, limits and an optional decision policy.

Effects: Anki reads only (`getActiveProfile`, `findNotes`, `notesInfo`, and `cardsInfo` for possible merge members); one immutable campaign definition, its frozen batches (phase `selected`) and the frozen policy file in local state. `--dry-run` stores nothing.

1. Validate the spec. A key set both by the creating command (`--set`) and by a source is refused (`CAMPAIGN_SETTING_DUPLICATE`): every child command would fail.
2. Read each source's notes; a note matched by an earlier source stays there.
3. Classify each note (first matching class): duplicate (same visible word and reading in a merging source, or a `merge_extra` group), split candidate, kana-only, missing recording or picture, plain. Merge groups keep the note with the most repetitions (then the oldest) as survivor.
4. Build batches in cohort order: the pilot first, then each cohort in batch-size chunks; one batch per merge group.
5. Freeze the creating command's `--config`, `--profile`, `--set` and `--offline` as the child commands' global arguments.

Result/failure: campaign ID, selection report (matched, per cohort, unordered, merge groups, batches), policy path. An empty selection is `CAMPAIGN_EMPTY`.

## OP-63 — `campaign list`

Inputs: none. Effects: local read. Result: campaigns, newest first, with status and pause code.

## OP-64 — `campaign status [CAMPAIGN]`

Inputs: campaign ID (default the newest). Effects: local read.

Result: status and pause code/detail; batches per phase and notes per phase; applied and deferred notes; `agent_needed` with one entry per batch that needs the agent (`needs_agent` notes and their open issues, `ready` batches with the notes to spot-check, batches with repeated step errors) and the commands that settle them; `user_needed` (paused); today's agent runs against the limit; the sync reminder.

## OP-65 — `campaign batch CAMPAIGN BATCH`

Inputs: campaign and batch. Effects: local read. Result: the batch row (phase, version, working data: notes, merge members, jobs, plan, deferred notes with reasons, approval, before-apply card facts, committed notes, verification problems, last error).

## OP-66 — `campaign events CAMPAIGN`

Inputs: `--after N`, `--limit` (1–1000). Effects: local read. Result: events oldest first (created, phase changes with detail, pauses, resumes, agent runs, pruned checkpoints).

## OP-67 — `campaign run [CAMPAIGN] [--max-seconds N]`

Inputs: campaign (default newest), tick budget (default 3000 s).

Effects: holds the campaign lease; runs this binary's own commands as child processes (prepare jobs, `plans resolve-batch --policy`, `plans generate --all-items --pending --certified-engine-only`, `plans merge-notes`, apply jobs with `--create-checkpoint`, `recover reconcile`, `apply --split-group`, `notes delete-merged`); reads companion note evidence; deletes checkpoint files only of checkpoints the campaign created (retention).

1. Stop at once unless the campaign is `running`.
2. The write lane (oldest batch in `approved`, `applying`, `verifying`, `merging`) and `prepare_parallel` prep workers loop on their own, each recording its steps with compare-and-set. A prep worker claims a batch for its step: the oldest unclaimed batch in `preparing`, `reviewing`, `needs_agent` whose step moves, or the next `selected` batch while fewer than `prepare_ahead` batches (claimed ones included) are in flight. Every step reads its batch fresh, so a batch another lane or command moved is left alone.
3. While an agent run is active (`agent-slot` recorded, no `agent-done` after it, under 45 minutes old) `needs_agent` batches are left alone, so a review never races the agent's edits.
4. A step that finds Anki, the companion or a service unavailable waits; a `recover reconcile` that ran but left the outcome unresolved (exit 4, Anki away a moment longer) is a step failure and is tried again, so only one that never resolves pauses the campaign; drafts wait while every generation provider is at its usage limit, and a local engine that no passed certification covers (an upgraded Ollama) pauses the campaign with `GENERATION_ENGINE_UNVERIFIED` instead of writing blocked drafts (`doctor --ollama --certify`, then `campaign resume`); repeated step errors pause the campaign; guardrail pauses stop the tick. A lane that panics stops the others.
5. After the pilot batch is `done`, pause with `CAMPAIGN_PILOT_REVIEW` once.
6. A lane with nothing to do looks again every 20 s while another lane works; a move on any lane wakes the others. Stop when every lane is idle at once, the budget is spent, or the campaign stopped running.

Result/failure: steps taken, why the tick stopped, pruned checkpoints. Exit 5 when the tick paused the campaign. `LEASE_HELD_OR_OWNER_UNVERIFIED` when another tick is running.

## OP-68 — `campaign approve CAMPAIGN BATCH --actor NAME`

Inputs: a `ready` batch; approver. Refused while the campaign is not running.

Effects: local plan revisions and approval; `plans bind` reads Anki.

1. Count notes whose review decisions include an actor other than the policy's (deferred notes excluded). Above `agent_share_max` (not for the pilot), pause with `CAMPAIGN_PAUSED_AGENT_SHARE` and refuse.
2. Bind, validate; a note with a validation error or open review is deferred.
3. Approve every remaining item with the warnings it carries accepted; a split group is approved whole or not at all.
4. Record the approval (items for the apply job, split groups, notes) and move the batch to `approved`.

## OP-69 — `campaign item CAMPAIGN BATCH NOTE --settled|--defer REASON|--undefer`

Inputs: a batch in `reviewing`, `needs_agent` or `ready`. Effects: local. `--settled` answers a split question; `--defer` leaves the note out of the batch (it stays unchanged in Anki; not allowed for merge members); `--undefer` puts it back. The batch returns to `needs_agent` so the next tick rechecks it.

## OP-70 — `campaign skip CAMPAIGN BATCH --reason R`

Inputs: a batch before approval. Effects: local; phase `deferred`. Nothing in Anki changes.

## OP-71 — `campaign pause CAMPAIGN [--reason R]`

Effects: local status `paused` with `CAMPAIGN_PAUSED_BY_REQUEST`. A running tick finishes its current step and stops.

## OP-72 — `campaign resume CAMPAIGN [--by-user]`

Effects: local status `running`. Refused without a terminal unless `--by-user` is given (`CAMPAIGN_RESUME_BY_USER_REQUIRED`): only the user resumes a paused campaign. The agent's allowlist excludes this command.

## OP-73 — `campaign cancel CAMPAIGN`

Effects: local status `cancelled` (terminal). Batches keep their state and history; applied notes stay applied.

## OP-74 — `campaign close CAMPAIGN`

Effects: local status `closed` (terminal); reminds the user to sync to AnkiWeb and choose **Upload**.

## OP-75 — `campaign agent-slot CAMPAIGN`

Effects: records one `agent_run` event when today's (UTC) count is below `agent_runs_per_day`; exit 5 and nothing recorded otherwise. Used by `scripts/campaign-agent.sh` before it starts an agent; the event also starts the agent hold (OP-67 step 3) until `campaign agent-done`.

## OP-76 — `campaign asset DIGEST --output FILE`

Effects: local read of one stored asset into a create-new file (so the agent can look at a picture or screenshot). `ASSET_NOT_FOUND` for an unknown digest.

## OP-77 — `campaign install-timer CAMPAIGN --script PATH --workdir DIR [--interval-minutes N] [--agent-script PATH] [--agent-interval-minutes N] [--execute]`

Effects: prints, or with `--execute` writes, `~/.config/systemd/user/linguist-campaign.{service,timer}`: a oneshot service running the tick script with `LAB_CAMPAIGN`, `LAB_BIN` and the working directory, and a persistent timer (`OnUnitInactiveSec`, default 15 minutes). With `--agent-script`, also `linguist-campaign-agent.{service,timer}`: the agent wrapper (`scripts/campaign-agent.sh`) on its own timer (default every 5 minutes, at most 40 minutes per run), so batches waiting for judgment do not wait for a tick to end. Enabling the timers is the user's `systemctl --user` command. `CAMPAIGN_INTERVAL_INVALID` for a zero interval.

## OP-78 — `jobs publish JOB --skip-failed`

Inputs: a prepare job whose pending items are all finished. Effects: holds the job lease; publishes the job's plan from its captured items only, leaving failed notes out. No Anki or provider read.

Result/failure: `plan_published`, the plan, and every skipped note with its error code; exit 5 when no item was captured.

## OP-79 — `plans merge-notes PLAN --item ITEM --note NOTE...`

Inputs: the survivor's plan item; duplicate note IDs. Effects: companion reads (note evidence, media bytes up to the asset limit); one child revision.

1. Read each duplicate's complete evidence (fields, tags, model, cards, review rows, media).
2. Add a `merged_note` source (`merged_anki_note:ID`) and an archive (the evidence record and the media bytes) to the survivor's item. Nothing of it is rendered.
3. Revalidate and publish the child revision; earlier decisions on the item reopen and are decided again.

## OP-80 — `notes delete-merged NOTE --survivor S --plan P [--apply]`

Inputs: a duplicate the plan merges into survivor S. Preview without `--apply`.

Effects (with `--apply`): one companion `delete_merged_note` effect.

1. Refuse unless the plan records the merge (`MERGE_NOT_RECORDED`), the duplicate still holds the archived fields (`MERGE_NOTE_CHANGED`), and the survivor is on the plan's rendered target note type (`MERGE_SURVIVOR_NOT_APPLIED`).
2. Record the operation ID, the content precondition and the duplicate's complete evidence (stored asset) before sending.
3. Send; the companion re-checks the precondition and the survivor's note type, deletes, and reads back that the note is gone.
4. A lost response is settled from the companion ledger on the next call; a refusal returns the record to `intent`.

## OP-81 — `notes restore-merged NOTE [--apply]`

Inputs: a deleted duplicate. Effects (with `--apply`): one companion `recreate_deleted_note` effect that inserts the note, its cards (same IDs, decks and schedule) and any review rows Anki no longer has, then recomputes the sort field and checksum. Refused when the note, a card or the GUID exists. The agent's allowlist excludes it.

## OP-82 — `campaign reopen CAMPAIGN BATCH --reason R [--fresh]`

Inputs: an `approved` batch with no apply job and no recorded apply intent. Effects: local. The approval and the before-apply card facts are dropped, deferrals made by the approval (`VALIDATION:`) are cleared, and the batch returns to `reviewing`; the next tick reviews it again and a new `campaign approve` is needed. The plan's approval record stays in history; no apply job can be created from it by the campaign. With `--fresh` (a `ready`, `needs_agent` or `approved` batch, no apply job, no apply intent) the batch returns to `selected` with only its notes, merge group, pilot flag and split candidates: the next tick creates a new prepare job and plan with the current settings (for example a stronger generation model), and notes it had deferred return too. The old plan stays in the store, unapproved.

## OP-83 — `campaign tune CAMPAIGN [--prepare-ahead N] [--prepare-parallel N] [--agent-runs-per-day N] [--agent-share-max X --by-user]`

Inputs: at least one throughput limit. Effects: local. Records a `tuned` event holding the new limits; the definition stays frozen and every later command reads the frozen limits with the newest tuning applied (`campaign status` shows them under `limits`). A running tick keeps the limits it started with. `--agent-share-max` (0.05-0.5) moves the agent share guardrail and is the user's step: it needs a terminal or `--by-user` (`CAMPAIGN_TUNE_BY_USER_REQUIRED`); the agent wrapper denies `campaign tune`. Other guardrails (`spot_check_share`, `max_step_failures`, review rounds) are not tunable. `CAMPAIGN_TUNING_INVALID` unless `prepare_parallel` is 1-8 and at most `prepare_ahead` (1-32) and `agent_runs_per_day` is at most 288; `CAMPAIGN_TUNING_EMPTY` without a limit; `CAMPAIGN_FINISHED` for a cancelled or closed campaign.

## OP-84 — `campaign agent-done CAMPAIGN`

Effects: local. Records an `agent_done` event, which ends the agent hold started by `agent-slot` (OP-67 step 3): batches waiting for the agent are reviewed again by the next tick step. `scripts/campaign-agent.sh` runs it after every agent run, whatever its exit status.

## OP-85 — `campaign requeue CAMPAIGN [--reason PREFIX]...`

Inputs: deferral reason prefixes (default `AGENT:` and `APPLY_FAILED:`). Effects: local, under the campaign lease (refused with `LEASE_HELD_OR_OWNER_UNVERIFIED` while a tick runs). For every `done` batch without a merge, the notes deferred for a matching reason move to one new `selected` batch (same cohort), prepared from scratch with the current settings (for example after a generation, prompt or policy fix). The finished batch keeps each note as deferred with `REQUEUED to batch N: <old reason>`, so a note is requeued once per pass and counted once in `campaign status`. Deferred notes were never written, so nothing in Anki changes. A `requeued` event names the notes. `CAMPAIGN_FINISHED` for a cancelled or closed campaign. The agent wrapper denies it: requeueing is the user's call.

## OP-86 — `campaign wrap-up CAMPAIGN [--wait-seconds N]`

Ends a session cleanly so the next one picks the campaign up where it stopped. Effects: local. A `running` campaign is paused (`CAMPAIGN_PAUSED_BY_REQUEST`, "wrapped up at the end of a session"); a campaign already paused keeps its reason (a guardrail, a quota). The command then waits up to `--wait-seconds` (default 900) for the running tick to finish its step and release the campaign lease; it never kills a tick, so an apply in progress completes under its journal. Once no tick runs, agent processes (`codex exec`, `claude -p`) left in their private `linguist-agent-*` directories by a generation process that is gone are ended with their process group. An open agent turn (`agent-slot` without `agent-done`) is closed. A `wrapped_up` event records the handoff (status, notes by phase, applied and deferred counts, batches waiting for the agent, providers cooling down after their usage limit, the commands to resume) and the command prints it. Exit 5 while a tick still runs after the wait; run it again. Resuming stays the user's step (`campaign resume`). `scripts/campaign-session.sh wrap-up` runs it after stopping the session's driver and watchers. The agent wrapper denies it.
