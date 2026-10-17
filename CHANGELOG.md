# Changelog

## Unreleased

Faster campaigns.

- **Independent lanes**: `campaign run` runs the write lane and `prepare_parallel` prep workers (1-8), each looping on its own; prep workers claim distinct batches and read each batch fresh before a step. A lane that panics stops the tick.
- **`campaign tune`**: change `prepare_ahead`, `prepare_parallel` and `agent_runs_per_day` of a running campaign (a `tuned` event; guardrails stay frozen). `campaign status` shows the effective `limits`.
- **Agent on its own timer**: `scripts/campaign-agent.sh` and `campaign install-timer --agent-script` (every 5 minutes). `campaign agent-slot` .. `campaign agent-done` hold `needs_agent` batches so a review never races the agent's edits; `campaign status` shows `agent_active`.
- **Quality fixes from the agent's spot checks**: the policy picks a sense only by an equal reading (台所 だいどころ no longer takes だいどこ; `に じゅう なな; に じゅう しち` reads as two readings, not syllables; readings on separate lines too); a draft whose examples repeat its collocations is refused (`GENERATION_EXAMPLE_NOT_SENTENCE`); a collocation that only holds the word's kanji inside a compound (体温 for 体 read からだ) and a nuance contrast with another spelling of the word (朝ご飯/朝御飯) are rejected; a draft whose explanations are not in the explanation language is refused (`GENERATION_EXPLANATION_LANGUAGE`); the vocabulary prompt states these rules.
- **Generated dictionary entries in prepare jobs**: the job passed an empty environment to the entry's provider, so an agent (Codex) could not be found (exit 127) and every phrase without a dictionary entry was left without a meaning; it now gets the job's environment (`dictionary` regeneration too).
- **Preparation**: `jobs publish --skip-failed` gives the plan a selection receipt naming exactly the captured notes (it was refused with `PLAN_SELECTION_INVALID`, and the campaign retried it without end); the campaign reads notes again that failed because Anki was busy or unreachable (after 5 minutes, up to 3 times) before leaving any out, and a refused publish counts as a step failure; a job whose attempt budget is spent is published without those notes at once; a step that moves a batch resets its failure count.
- **Generation**: a draft keeps the item's decisions about the source alone (a source image's role, verified source or OCR text), rebound to the new document, so an agent's image decision no longer reopens after the next draft; a new draft drops the engine markers of the item's earlier drafts (a stale `GENERATION_ENGINE_UNVERIFIED` from a local fallback no longer blocks the item); `plans generate --all-items --pending` drafts an item at most 3 times (`GENERATION_ATTEMPTS_EXHAUSTED`).
- **`llm.agents.codex.reasoning_effort`** (default, minimal, low, medium, high, xhigh): passed to Codex as `-c model_reasoning_effort`; `default` keeps the Codex configuration's own. An example sentence on a Japanese, Chinese or Korean card must contain that script (`GENERATION_EXAMPLE_LANGUAGE`; a draft gave `x = x`).
- **Agent timeouts end the whole agent**: an agent runs in its own process group and a timeout kills the group, so a wrapper's child (`codex-meter.sh` -> `codex exec`) no longer outlives its deadline.
- **More spot-check findings in code**: a nuance contrast with one of the word's own forms (節約 and 節約する) is rejected; a draft that copies the prompt's placeholder ("the accepted expression", or "the expression" on a vocabulary card) is refused (`GENERATION_PROMPT_WORDING`); the vocabulary prompt asks for the word by name.
- **A batch whose drafts failed is never ready**: `plans generate` errors (a usage limit, a provider down, a draft the checks refused) were read as "nothing new to draft", so a batch could reach `ready` without examples, collocations or nuance. A usage limit now waits; any other failure drafts the note again, up to 3 times, then leaves it to the agent (`GENERATION_FAILED`).
- **Source decisions survive a draft and a sense choice**: the draft path cleared every review after the merge had kept the item's decisions about the source alone, and a sense choice (which the policy makes again after every draft) cleared them too, so an agent's image role and classification went back to the agent after each draft; both now keep the source-only decisions, rebound to the new document while they still settle their issue.
- **`campaign reopen --fresh`**: a ready, needs_agent or approved batch without an apply job is prepared again from scratch with the current settings.
- **`campaign requeue`**: notes that finished batches deferred (default reasons `AGENT:`, `APPLY_FAILED:`) go round again in new batches.
- **Fallback through a usage limit**: `llm.quota_cooldown_seconds` (default 1800): an agent or API that reports its usage limit is skipped for that long (`AGENT_QUOTA_COOLDOWN`, a marker under `storage.state_dir/agents/` shared by every process), so the next provider of `llm.fallback` (for example local Ollama) drafts at once; then it is tried again. Drafts go one at a time while that leaves Ollama first. A campaign waits for the quota only when every provider of the chain hit its usage limit; a fallback that failed otherwise counts as a failed draft.
- **A collocation gloss copied from an example translation is refused** (`GENERATION_COLLOCATION_GLOSS`; 整う got full sentences as glosses).
- **Examples use the word; affixes keep theirs**: an example with no form of the word (直ちに drew a sentence with たちまち) is rejected; the compound rules skip prefixes, suffixes and counters (副, 総, 代), which are only ever written inside compounds and had every example rejected.
- **Source decisions survive every content change**: an image's role and verified source or OCR text are rebound after any decision that changes the document (a rejected generated fact, a chosen media candidate), not only after a sense choice; the policy rejecting a collocation had sent an agent's image role back to the agent.
- **`campaign tune --agent-share-max X --by-user`**: the agent share guardrail (0.05-0.5) moves only as the user's step (a terminal or `--by-user`); image-heavy batches tripped the 20% default with dictionary screenshots the agent archived.
- **A batch never waits on needs already met**: a batch waiting for the agent is reviewed again when every need it lists is a review issue the plan no longer holds open (a review that raced the agent's decisions recorded needs already met).
- **No stale agent needs**: a review stamps the revision its undecided list describes (the policy pass's own), not a later read; an agent decision that landed while the review ran makes a newer revision, so the batch is reviewed again instead of waiting for the agent on needs already met (the `--settled` workaround is no longer needed).
- **An unresolved reconcile is tried again**: `recover reconcile --apply` exit 4 (it ran, the outcome is still unresolved, for example Anki away a moment longer) is a step failure retried by the next step instead of an immediate `CAMPAIGN_PAUSED_UNKNOWN_OUTCOME`, and its outcome (stdout) is named instead of an empty detail.
- **Compounds are not the word**: a noun written with trailing kana is part of a compound when a kanji or katakana noun follows it (物干し台, 物干しロープ for 物干し), and an example whose kanji word appears only inside compounds (球体, 球場 for 球) is rejected.
- **Hangul on a Japanese card is refused** (`GENERATION_SCRIPT_MIXED`; 決して drew 絶대로 for 絶対に) and the note is drafted again.
- **A collocation that is one of the word's own forms is rejected** (遠慮する for 遠慮), as a nuance contrast already was.
- **Agent call budget**: `llm.agents.max_calls_per_window` (0 = none) and `llm.agents.budget_window_seconds` (default 18000): an agent or API that started that many calls in the window is skipped like one at its usage limit until the window frees (a call log under `storage.state_dir/agents/`), so `llm.fallback` drafts meanwhile and a quota shared with other work is never run out.
- **Campaigns never draft with an uncertified local engine**: `plans generate --pending --certified-engine-only` (the campaign passes it) leaves out a local Ollama engine that no passed certification covers (identity and version; an upgraded Ollama loses it), and refuses each item with `GENERATION_ENGINE_UNVERIFIED` when that engine is the provider drafts go to now. The campaign pauses with that code instead of writing drafts that approval would refuse; `scripts/campaign-session.sh` certifies again and resumes when the certification passes.
- **`campaign wrap-up`**: ends a session cleanly: pauses the campaign (a campaign already paused keeps its reason), lets the running tick finish its step, ends agent processes left by a generation process that is gone, closes an open agent turn and records a `wrapped_up` handoff event (exit 5 while a tick still runs). `scripts/campaign-session.sh start|watch|status|wrap-up` runs a campaign from a session: it starts VOICEVOX when needed (never Anki), resumes a wrapped-up campaign, drives ticks back to back, and on wrap-up stops its driver, watchers and what it started.
- **Parallel generation**: `llm.agents.parallel` (1-8, default 1) drafts that many items at once from an external provider in `plans generate --all-items --pending`; local Ollama stays one at a time.

## 0.3.0 — 2026-10-11

Unattended collection revamp campaigns (WP-24). Companion add-on 0.5.0; local store schema 15.

- **Campaigns**: `campaign create|list|status|batch|events|run|approve|item|skip|pause|resume|cancel|close|agent-slot|asset|install-timer`. A campaign freezes a selection into cohorts and batches and advances them one tick at a time through prepare jobs, policy review, generation, approval, apply jobs with their own checkpoint, verification of every card and review row, and checkpoint retention. All state is in the store; a killed tick, a restarted Anki or a new agent session continues where the last stopped. Only the user resumes a paused campaign.
- **Decision policy**: `plans resolve-batch --policy FILE|builtin` decides what the user's standing preferences settle (native history map, sense by reading, pictures by the OCR image classification, own recordings, generated facts) and lists every issue it leaves open; `resources/policies/campaign-default.json`.
- **One revision per pass**: `plans resolve-batch` publishes one revision per pass instead of one per decision.
- **Resumable generation**: `plans generate --all-items --pending` skips items with nothing to generate.
- **Partial preparation**: `jobs publish JOB --skip-failed` publishes a prepare job's plan without its failed notes.
- **Duplicate merge**: `plans merge-notes` archives duplicates on their survivor's item; `notes delete-merged` deletes a duplicate after its survivor is applied (companion `delete_merged_note`); `notes restore-merged` recreates it with its note, card and review IDs (`recreate_deleted_note`).
- **Native history**: a uniform history map drops entries for templates a note has no card of.
- **Agent driver**: `scripts/campaign-tick.sh` (systemd user timer) and the `campaign-tick` project skill.
- Release check 2026-10-12: every command passes, including all eleven desktop scenarios; 13 of 14 gates pass, EV-02 stays blocked.

## 0.2.1 — 2026-10-11

- `--help` and error guidance describe companion writes as available: `apply`, `snapshots`, `jobs`, `recover`, `plans`, `doctor`, `models` and `backup` no longer say writes are "unavailable until the native adapter is verified". The `CAPABILITY_UNAVAILABLE` hint names the released `linguist-bridge-*.ankiaddon` and Anki 25.09.2.
- Work-package records name the release-check, traceability and check-dev scripts on the `cli-dev` branch.
- Full release check rerun on the split repository ([evidence](https://github.com/silam741852963/linguist-anki-bridge/blob/cli-dev/docs/cli/evidence/release-2026-10-11-v0.2.1/README.md)): every command passes, 633 workspace tests and all ten desktop scenarios; 13 of 14 gates pass, EV-02 stays blocked (`EV02_NONDEFAULT_CONSUMER_TESTS_INCOMPLETE`).

## 0.2.0 — 2026-10-11

First release with collection writes. Companion add-on 0.4.0.

- **Writes through the companion add-on** (`lab-native-v1`): `apply --apply` for items and split groups, `snapshots restore --apply`, `jobs rollback --apply`, `recover reconcile --apply [--rebind]`, `backup create --apply` and `models install --apply`, each journaled under the collection-writer lease after a verified native checkpoint. `plans bind` records the collection binding before approval (WP-17).
- **Revamp review**: `plans resolve-history` maps source cards from live companion evidence; `plans resolve-batch` reviews a whole plan from one decisions file (WP-17, WP-21).
- **Jobs**: full-pipeline prepare jobs with per-item checkpoints; apply jobs over the companion with one checkpoint per job, crash resume through reconcile and group rollback (WP-21).
- **Card models**: Linguist Vocabulary v3 (stroke GIFs, dictionary audio, comprehension/production/spelling), English Vocabulary v1, and Linguist Grammar v4 (Usage and Nuance fields, per-example audio, no exercise card) (WP-19, WP-22, WP-23).
- **Splits**: vocabulary and grammar splits; new split units inherit the source card's schedule (WP-20, WP-23).
- **Local engine certification**: `doctor --ollama --certify` (WP-19).
- **Release check 2026-10-11**: 13 of 14 gates pass on Anki 25.09.2 disposable collections; EV-02 is blocked (`EV02_NONDEFAULT_CONSUMER_TESTS_INCOMPLETE`).
- **Repository**: the archived GUI/TUI code moved to the `legacy` branch; release evidence, reviews, decisions and research moved to the `cli-dev` branch. The gate registry is now `resources/release-gates.json`.

## 0.1.0 — 2026-10-05

Headless CLI: prepare, review, validate, approve and export vocabulary and grammar cards; read Anki through AnkiConnect; durable local plans, jobs, snapshots and recovery records. Collection writes were preview-only.
