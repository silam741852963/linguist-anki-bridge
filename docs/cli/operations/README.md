# Operation catalogue

Status: target command interface. Every public operation has an ID and explicit algorithm. Smaller models should implement one entry with its referenced algorithms and tests; do not infer mutation from command names. `lab` is the documented alias for `linguist-anki-bridge`, not a required executable.

## Shared command wrapper

1. Parse clap arguments; help/no-subcommand/completions return before state initialization. Invalid syntax exits 2.
2. Resolve ALG-CONFIG only for needed capabilities. Initialize private local state only for operations that use it; read-only commands do not write Anki. Select an exact revision when approval/apply/recovery depends on one.
3. Validate all request inputs and safety flags before calls. No command accepts a global force/auto-apply. `--apply` authorizes Anki mutations in that invocation only; `--execute` confirms previewed local destructive changes. `--yes` does not resolve content decisions.
4. Return stable structured diagnostics and actionable next command. stdout contains result text or versioned JSON/JSONL; progress/logs go to stderr. Never mix private diagnostic payloads into machine output.
5. Exit codes: 0 complete/read/empty/no-op; 2 usage/config/schema; 3 unavailable dependency; 4 review needed; 5 source/identity/conflict; 6 provider/ordinary execution failure; 7 unresolved recovery/partial mutation; 130 interrupted after durable accounting. For mixed batches choose 7 > 5 > 4 > 6 > 3 > 2; show all per-item codes. Broken pipes are handled without noisy traceback.
6. On Ctrl-C, stop dispatch, cancel safe reads, and finish durable accounting for sent mutations. If that cannot finish, leave unknown journal and exit 7 (or 130 with explicit pending recovery ID); never delete recovery evidence.

Selectors: `--note-id` repeatable OR `--query` OR `--deck`/`--purpose` source mapping; conflicting selector modes fail. Add inputs: inline text OR --input file OR --document structured file. Explicit input format distinguishes one multiline record from JSONL/CSV multiple records. Writes show notes/cards/tasks separately. File exports create-new by default, --replace applies only to that output after backup.

Aliases `inject`/`modernize` may map to add/revamp with deprecation messages; they use identical IDs/authorization. No hidden GUI command starts a service. See [pipelines](../pipelines/README.md), [recovery](../recovery/README.md) and [configuration](../configuration/README.md).


## Operation lookup

| ID | Command |
| --- | --- |
| OP-01 | [`doctor`](doctor.md#op-01-doctor) |
| OP-02 | [`config init`](configuration.md#op-02-config-init) |
| OP-03 | [`config show`](configuration.md#op-03-config-show) |
| OP-04 | [`config describe KEY`](configuration.md#op-04-config-describe-key) |
| OP-05 | [`config validate`](configuration.md#op-05-config-validate) |
| OP-06 | [`config set KEY VALUE`](configuration.md#op-06-config-set-key-value) |
| OP-07 | [`config unset KEY`](configuration.md#op-07-config-unset-key) |
| OP-08 | [`config reset`](configuration.md#op-08-config-reset) |
| OP-09 | [`config import`](configuration.md#op-09-config-import) |
| OP-10 | [`config migrate`](configuration.md#op-10-config-migrate) |
| OP-11 | [`decks list`](collection.md#op-11-decks-list) |
| OP-12 | [`decks show DECK`](collection.md#op-12-decks-show-deck) |
| OP-13 | [`decks map PURPOSE`](collection.md#op-13-decks-map-purpose) |
| OP-14 | [`decks unmap PURPOSE`](collection.md#op-14-decks-unmap-purpose) |
| OP-15 | [`models list`](collection.md#op-15-models-list) |
| OP-16 | [`models inspect MODEL`](collection.md#op-16-models-inspect-model) |
| OP-17 | [`models install PURPOSE`](collection.md#op-17-models-install-purpose) |
| OP-18 | [`notes list`](collection.md#op-18-notes-list) |
| OP-19 | [`notes show NOTE_ID`](collection.md#op-19-notes-show-note_id) |
| OP-20 | [`notes count`](collection.md#op-20-notes-count) |
| OP-21 | [`vocab add`](preparation.md#op-21-vocab-add) |
| OP-22 | [`vocab revamp`](preparation.md#op-22-vocab-revamp) |
| OP-23 | [`grammar add`](preparation.md#op-23-grammar-add) |
| OP-24 | [`grammar revamp`](preparation.md#op-24-grammar-revamp) |
| OP-25 | [`plans list`](plans.md#op-25-plans-list) |
| OP-26 | [`plans show PLAN`](plans.md#op-26-plans-show-plan) |
| OP-27 | [`plans diff PLAN`](plans.md#op-27-plans-diff-plan) |
| OP-28 | [`plans edit PLAN`](plans.md#op-28-plans-edit-plan) |
| OP-29 | [`plans resolve PLAN ISSUE`](plans.md#op-29-plans-resolve-plan-issue) |
| OP-30 | [`plans regenerate PLAN`](plans.md#op-30-plans-regenerate-plan) |
| OP-31 | [`plans validate PLAN`](plans.md#op-31-plans-validate-plan) |
| OP-32 | [`plans export PLAN`](plans.md#op-32-plans-export-plan) |
| OP-33 | [`plans approve PLAN`](plans.md#op-33-plans-approve-plan) |
| OP-34 | [`apply PLAN`](apply.md#op-34-apply-plan) |
| OP-35 | [`jobs create`](jobs.md#op-35-jobs-create) |
| OP-36 | [`jobs list`](jobs.md#op-36-jobs-list) |
| OP-37 | [`jobs show JOB`](jobs.md#op-37-jobs-show-job) |
| OP-38 | [`jobs items JOB`](jobs.md#op-38-jobs-items-job) |
| OP-39 | [`jobs run JOB`](jobs.md#op-39-jobs-run-job) |
| OP-40 | [`jobs pause JOB`](jobs.md#op-40-jobs-pause-job) |
| OP-41 | [`jobs resume JOB`](jobs.md#op-41-jobs-resume-job) |
| OP-42 | [`jobs retry JOB`](jobs.md#op-42-jobs-retry-job) |
| OP-43 | [`jobs cancel JOB`](jobs.md#op-43-jobs-cancel-job) |
| OP-44 | [`jobs rollback JOB`](jobs.md#op-44-jobs-rollback-job) |
| OP-45 | [`jobs delete JOB`](jobs.md#op-45-jobs-delete-job) |
| OP-46 | [`jobs audit JOB`](jobs.md#op-46-jobs-audit-job) |
| OP-47 | [`jobs migrate`](jobs.md#op-47-jobs-migrate) |
| OP-48 | [`snapshots list`](history.md#op-48-snapshots-list) |
| OP-49 | [`snapshots show SNAPSHOT`](history.md#op-49-snapshots-show-snapshot) |
| OP-50 | [`snapshots restore SNAPSHOT`](history.md#op-50-snapshots-restore-snapshot) |
| OP-51 | [`snapshots export SNAPSHOT`](history.md#op-51-snapshots-export-snapshot) |
| OP-52 | [`backup create`](history.md#op-52-backup-create) |
| OP-53 | [`backup list`](history.md#op-53-backup-list) |
| OP-54 | [`backup verify BACKUP`](history.md#op-54-backup-verify-backup) |
| OP-55 | [`cache status`](maintenance.md#op-55-cache-status) |
| OP-56 | [`cache prune`](maintenance.md#op-56-cache-prune) |
| OP-57 | [`resources list`](maintenance.md#op-57-resources-list) |
| OP-58 | [`resources install RESOURCE`](maintenance.md#op-58-resources-install-resource) |
| OP-59 | [`recover inspect`](maintenance.md#op-59-recover-inspect) |
| OP-60 | [`recover reconcile OPERATION`](maintenance.md#op-60-recover-reconcile-operation) |
| OP-61 | [`completions SHELL`](maintenance.md#op-61-completions-shell) |
