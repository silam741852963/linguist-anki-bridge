# Configuration operations

Read [shared command rules](README.md) before implementing any handler.

## OP-02 — `config init`

Inputs: Optional --path; --replace only with existing-file backup.

Effects: Local config file only.

1. Write minimal config.version=2 so builtin defaults/purpose presets remain inherited; do not persist every default as an override.
2. Validate ALG-CONFIG; show required deck mappings/resources and selected local model candidate, without installing them.
3. Atomically write private TOML; existing file is protected by default.

Result/failure: Path and next config/deck commands; no service calls. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-03 — `config show`

Inputs: --defaults, --effective, --provenance, optional KEY.

Effects: Read-only.

1. Load defaults or ALG-CONFIG effective view.
2. Select exact key/prefix; reject unknown selector.
3. Redact secrets/private references appropriately and serialize typed values plus provenance.

Result/failure: Versioned settings; config show --defaults works without dependencies. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-04 — `config describe KEY`

Inputs: One registered concrete/pattern key.

Effects: Read-only.

1. Look up registry entry; resolve valid wildcard names if supplied.
2. Print type/default/range/scope/consumer and required cross-field checks.

Result/failure: Unknown key gives usage error and nearest valid names. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-05 — `config validate`

Inputs: Candidate --file or effective settings.

Effects: Read-only except optional report export.

1. Parse source format/version.
2. Run structural and cross-field ALG-CONFIG checks.
3. Report missing runtime resources separately from malformed config; no download or installation.

Result/failure: All diagnostics with key paths; nonzero on invalid config. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-06 — `config set KEY VALUE`

Inputs: Optional --profile/--purpose; typed value.

Effects: Local config edit.

1. Resolve key/type/scope; parse value without shell evaluation.
2. Construct candidate retaining other values; validate entire merged configuration.
3. Atomically replace with backup copy; active storage relocation is blocked.

Result/failure: Changed value/provenance, affected future stages; existing plans unchanged. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-07 — `config unset KEY`

Inputs: Exact override scope; no global force.

Effects: Local config edit.

1. Require existing registered override.
2. Remove override so inheritance applies; show effective replacement.
3. Validate and atomically save; nonexistent override returns idempotent no-op.

Result/failure: Effective new value; frozen plans unchanged. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-08 — `config reset`

Inputs: Explicit KEY or --all, candidate diff; --execute to save.

Effects: Local config edit only.

1. Select scope and calculate removal/default diff.
2. Preview effects and required unresolved choices.
3. With --execute validate/save backup atomically; never delete state/jobs/secrets resources.

Result/failure: Preview or receipt; configuration reset does not grant apply permission. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-09 — `config import`

Inputs: Legacy --file and candidate --output; --replace scoped output only.

Effects: Local candidate/resource files.

1. Detect supported Python YAML/native JSON source version.
2. Account for every source key using migration table; export prompts/schemas to hashed files.
3. Validate candidate; preserve unresolved decisions in report.
4. Write candidate/report without modifying source or activating it automatically.

Result/failure: Imported/transformed/unresolved/retired keys; unresolved unsafe mappings block activation. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-10 — `config migrate`

Inputs: Existing target version and --output.

Effects: Local candidate files.

1. Read known version and immutable backup copy.
2. Run ordered pure version migrations; future/unknown version rejected.
3. Validate and export diff/candidate; explicit --execute needed to activate atomically.

Result/failure: Migration receipt; same-version idempotent no-op. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.
