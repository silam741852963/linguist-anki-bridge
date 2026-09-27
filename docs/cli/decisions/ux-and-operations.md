# Final command and setup decisions

Owner: WP-02/WP-05/WP-09/WP-13/WP-15. Public command IDs remain OP-01–OP-61; no extra command families are needed for baseline.

## Setup and defaults

Canonical executable is linguist-anki-bridge. Development uses `cargo run -p linguist-cli -- …`, avoiding collision with installed legacy Python command; no second permanent preview executable. Initial supported runtime is Linux/Arch, headless CLI plus external Anki Desktop; other operating systems require their own filesystem/locking/native compatibility tests before advertised support.

`config init` writes minimal `config.version=2`, preserving inherited defaults/presets, not every default as an override. `config show --defaults` and the full example expose all keys. Global builtin default plus builtin purpose presets resolve before user base/profile/purpose/env/flags. Purpose presets are data in configuration/purpose-defaults.json, never hidden code branches. Actual deck names are not builtin defaults.

No implicit deck creation in first release. `decks map` accepts existing regular source/target decks; missing target gives a clear Anki setup instruction. Revamp can omit target to retain home decks; add cannot. Models install is explicitly journaled; no new shared model overwrite. Completing config does not grant write permission.

## Editing and source review

`plans edit` supports --patch file OR --editor, with explicit base revision/digest. Editor mode writes a private typed draft file, launches configured editing.editor_argv or safely parsed VISUAL/EDITOR, waits, then parses/validates a new revision. No shell interpolation; unsupported shell constructs error. Empty editor configuration without VISUAL/EDITOR returns a remedy instead of launching an arbitrary editor. Abort leaves prior revision unchanged; edited invalid content can be saved only as a marked draft with issues. Models/providers never choose the editor.

Keep source text/images and user edits; generator overwrites only an explicit selected user-field list. Inline plain input is one record; JSONL/CSV multiple records are explicit. Repeated identical input rows report skip_exact within one frozen selection; same expression with different sense/context is not an exact duplicate. Add never becomes revamp merely because Anki's duplicate detection found a match.

Missing Anki bridge/checkpoint is preflight capability failure, not a reason to make content endlessly needs_review. Thus plans can be reviewed/exported offline, while apply remains blocked. Modernization completeness is receipt/task/model-manifest evidence, independent of Anki new/reviewed scheduling status. No undocumented scheduling-based selector masquerades as content completion.

## Approval and jobs

`plans approve` binds content digest. `apply --apply` can record approval of the displayed ready revision for selected items and accepted warning codes, but cannot resolve a review issue. `--yes` is unnecessary for noninteractive scripts that supply exact revision/digest/accepted warnings; it grants nothing extra. Empty selection is successful no-op.

Job modes remain immutable. To turn prepared outputs into writes, create a separate apply job referencing exact approved plan revisions. Pause/cancel sets controls and stops new dispatch; active native requests remain queued/running until their recorded result is known. Apply resume/retry always requires current --apply. After an Anki collection-session change, --rebind is an explicit evidence-bound continuation, not force. Cancellation is never rollback.

Structured output schema includes command/request IDs, status, counts by inputs/items/notes/cards, per-item issues, receipt/recovery IDs and next_command. stdout is result only; progress goes to stderr. Partial/no-op results use explicit counts; source text/secrets are omitted by default. CLI conventions support discoverable help and predictable piping; the concrete policy here is app-specific. [CLI Guidelines](https://clig.dev/).

## Backup and sync usability

Migration, reverse migration and shared model creation require one collection-wide .colpkg checkpoint including media at the recovery-group boundary. All content-only updates/adds require verified affected-scope scheduling/media coverage plus immutable per-item snapshots. One group checkpoint can protect many items only with a scope-closed frozen selection; every item still captures fresh current pre-state. Do not export the whole collection for each note. backup.scope preference cannot reduce mandatory coverage.

Deck .apkg exports are not selective rollback: re-import merge/modification rules may preserve newer data; collection .colpkg import replaces the collection. Normal restore uses snapshots/native mappings; full import remains manual disaster recovery. These distinctions follow [Anki export semantics](https://docs.ankiweb.net/exporting.html).

Anki note-type changes can require full sync. Report the schema/sync impact in preview and receipt; record explicit warning acceptance. Native schema confirmation must be handled through tested official semantics, never by bypassing a required confirmation. Do not trigger sync, choose upload/download direction or silently disable sync. [Anki browsing documentation](https://docs.ankiweb.net/browsing.html) identifies the note-type/full-sync consequence. This app does not own multi-device synchronization.
