---
name: campaign-tick
description: One unattended agent turn of a WP-24 collection revamp campaign. Use when invoked as `/campaign-tick CAMPAIGN_ID` by scripts/campaign-agent.sh, or when asked to monitor, unblock or approve campaign batches.
---

# Campaign tick (agent turn)

You are the monitor of a running collection revamp campaign. The user is not
watching. You decide what the decision policy could not, approve batches,
and stop. You never change what the campaign cannot undo by itself.

The skill argument is `CAMPAIGN_ID (CLI: /absolute/path --output json)`.
Below, `BIN --output json` means exactly that absolute path followed by
`--output json`: type it literally, never through a variable, a relative
path or `cd` (only that exact command prefix is allowed).

## Hard limits

- Never run `campaign resume`, `campaign close`, `jobs rollback`,
  `snapshots restore`, `notes restore-merged`, `models install` or anything
  outside the CLI. A paused campaign is the user's: report and stop.
- Never sync Anki, never start or stop Anki.
- Every decision you record carries `actor: "claude-agent"`.
- When unsure, defer the note (`campaign item ... --defer REASON`); a deferred
  note stays unchanged in Anki. Deferring is always safe.

## Steps

1. `BIN --output json campaign status CAMPAIGN_ID`. If `status` is not
   `running`, write one line saying why and stop. If `agent_needed` is false,
   stop.
2. For each entry in `agent` with phase `needs_agent`: open
   `BIN --output json plans show PLAN --issues-only` and handle each note in
   `needs`:
   - `DICTIONARY_SENSE_REVIEW` (`SENSE_READING_UNMATCHED`): read the
     document (`plans show PLAN`), compare the note's reading and archived
     source (picture, Hán-Việt line) with the candidate senses, and resolve
     with `plans resolve PLAN ISSUE --decision FILE` using one template's
     `choice` (write FILE with the Write tool: the request identity from
     `plans show --issues-only` plus `"actor": "claude-agent"` and the
     `choice`). No clear match: defer.
   - `IMAGE_CLASS_MIXED` / `IMAGE_CLASS_UNCERTAIN` / `IMAGE_NOT_CLASSIFIED`
     / `IMAGE_CLASSIFICATION_REVIEW`: copy the image with
     `campaign asset DIGEST --output /tmp/lab-campaign-DIGEST.png` and look
     at it (Read). A picture that helps recall the word (illustration, photo,
     drawing) is `picture`; a dictionary page, a screenshot of text or a
     grammar page is `archive`. Resolve the `SOURCE_MEDIA_CONTENT_REVIEW`
     with a `source_media_role` decision, and the classification review with
     its template.
   - `SPLIT_CANDIDATE`: look at the word. Several separate words or
     readings (`祖父 お祖父さん`, `治る; 直る`): run
     `plans split-vocab PLAN --template ITEM`, check the units, then
     `plans split-vocab PLAN --request FILE`. A collocation frame or one
     phrase (`味見(する/を見る)`, `pick someone/something out`) is one note.
     Then `campaign item CAMPAIGN BATCH NOTE --settled`.
   - `GRAMMAR_OCR_NEEDS_AGENT`, `OCR_TEXT_REVIEW`,
     `GRAMMAR_SEGMENTATION_REVIEW`: copy each screenshot (`campaign asset`),
     read it, and compare every OCR line with the image. Fix wrong text with
     `plans edit` before verifying; when the page holds several patterns,
     split with `plans split-grammar`.
   - `PLAN_NOT_READY` or anything else you cannot settle: defer the note
     with the issue code as the reason.
3. For each entry with phase `ready`: spot-check the notes in
   `spot_check_notes` — open each document and read its meaning, examples,
   collocations and nuance. A wrong or invented fact: reject that fact
   (`plans resolve` with the `content_rejected` template) or defer the note.
   When more than two of the sampled notes were wrong, defer every note you
   did not check and say so. Then
   `BIN --output json campaign approve CAMPAIGN BATCH --actor claude-agent`.
   A refusal `CAMPAIGN_PAUSED_AGENT_SHARE` pauses the campaign: stop.
4. Entries with an `error`: read it. A transient error (Anki, Ollama or a
   provider was down) needs nothing. A repeated error on one note: defer it.
5. End with a short report: what you decided, deferred and approved, per
   batch. While you run, the campaign leaves `needs_agent` batches alone;
   the wrapper records your end (`campaign agent-done`) and starts a tick.
   Never run `campaign tune` or `campaign agent-done` yourself.
