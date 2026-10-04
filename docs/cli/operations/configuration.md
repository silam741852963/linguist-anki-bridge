# Configuration operations

Read [shared command rules](README.md) before implementing any handler.

## OP-02 — `config init`

Inputs: Optional --path; --replace only with existing-file backup.

Effects: Local config file only.

1. Write minimal config.version=2 so builtin defaults/purpose presets remain inherited; do not persist every default as an override.
2. Validate ALG-CONFIG; show required deck mappings/resources and selected local model candidate, without installing them.
3. Atomically write private TOML; existing file is protected by default.

Result/failure: Path and next config/deck commands; no service calls. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

The current receipt includes per-purpose missing add/revamp mapping keys, language, model candidate and required local resource gaps. Candidate settings are checked before writing; model/resource verification is still pending. The planned deck mapping command is labelled separately from commands currently available.

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

Current output retains the registered type/default/range/scope/consumer fields and adds `resolved_key` plus `cross_field_checks`. The checks cover current numeric relations, provider prerequisites and remote-host policy. Typo suggestions are bounded to three names and do not load the selected config.

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

Current edit receipts include `changes` for every override changed by set, unset or reset. Each entry gives old/new override, old/new effective value and provenance. A reset preview calculates inherited replacement values and leaves the config bytes untouched; `--execute` publishes the same validated candidate with a backup.

## OP-09 — `config import`

Inputs: Legacy --file and candidate --output; --replace scoped output only.

Effects: Local candidate/resource files.

1. Detect supported Python YAML/native JSON source version.
2. Account for every source key using migration table; export prompts/schemas to hashed files.
3. Validate candidate; preserve unresolved decisions in report.
4. Write candidate/report without modifying source or activating it automatically.

Result/failure: Imported/transformed/unresolved/retired keys; unresolved unsafe mappings block activation. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current behavior (WP-14): `config import --file LEGACY --output CANDIDATE.toml [--replace]` detects native JSON v1 or Python YAML (`config_version` 1/2 or absent; other versions fail). YAML is parsed by a restricted loader that refuses tags, anchors, aliases, merge keys, complex or duplicate keys, multiple documents and ambiguous YAML 1.1 scalars (octal, hexadecimal, sexagesimal, dates, non-finite numbers). Every source key gets one report record (`transferred`, `transformed`, `defaulted`, `unresolved`, `retired`, `unsupported`, `unknown`); keys absent from a Python file are also reported with `implicit_default` when the legacy runtime default applied. Overrides are written only where the value differs from the v2 builtin or purpose-preset value. Prompts and schemas are exported byte-exactly to `CANDIDATE.toml.resources/SHA256.{txt,json}`; legacy defaults are retired in favour of builtin v2 resources, custom ones stay unresolved. Blocking records (unknown keys, custom prompts/schemas, conflicting retry or endpoint values, enabled unavailable features, rejected values, configured unsupported purposes) are listed in `blocking_keys`; exit 4 when any exist. The report (`CANDIDATE.toml.import.json`) and candidate are private files; the source and the live config path are refused as outputs, and `--replace` replaces only this import's own files. `--set`, `--profile`, `--purpose` and `--offline` are refused.

## OP-10 — `config migrate`

Inputs: Existing target version and --output.

Effects: Local candidate files.

1. Read known version and immutable backup copy.
2. Run ordered pure version migrations; future/unknown version rejected.
3. Validate and export diff/candidate; explicit --execute needed to activate atomically.

Result/failure: Migration receipt; same-version idempotent no-op. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current behavior (WP-14): with a v2 file, `config migrate` is the `already_current` no-op. `config migrate --from-import CANDIDATE [--accept-unresolved KEY]... [--execute]` activates an imported candidate: the candidate must match the digest in its report, every blocking key must be accepted explicitly (unknown acceptances fail), and the whole candidate is validated for every builtin purpose and profile. Without `--execute` it previews added, removed and changed keys. With `--execute` it locks the config, copies an existing live file byte-exactly to a private backup, refuses state relocation while state is nonempty, and publishes atomically (create-new when no live file exists). No version ladder beyond v2 exists yet.
