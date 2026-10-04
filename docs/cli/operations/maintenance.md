# Maintenance operations

Read [shared command rules](README.md) before implementing any handler.

## OP-55 — `cache status`

Inputs: Optional provider/purpose.

Effects: Local read.

1. Compute cache totals, reachable/protected/unreferenced bytes and retention policy.

Result/failure: No asset deletion. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current behavior (WP-14): `cache status [--provider SERVICE]` reports provider-cache entries per service (intact, incomplete/corrupt, older than retention), unmanaged paths, and for existing state the store-asset census: reachable/protected bytes by root table, unreferenced, stray and temporary files, indexed-but-missing assets, active leases, pending recovery journals, interrupted prune runs and the blockers. It never creates state.

## OP-56 — `cache prune`

Inputs: Optional age/budget; --execute to delete local candidates.

Effects: Preview/local unreferenced cache deletion.

1. Run ALG-GC reachability and candidate preview.
2. With --execute lock/recheck and remove only safe unreferenced cache.

Result/failure: Prune receipt; no Anki media/history deletion. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current behavior (WP-14): `cache prune [--age-days N] [--budget-mb N] [--provider SERVICE] [--execute]` previews by default. Store reachability is conservative: any 64-hex run in any row (except the asset index and GC bookkeeping) marks an asset, and marked assets are scanned transitively. Unreferenced files younger than one hour are never deleted. Any active lease or pending journal blocks the whole prune (`GC_BLOCKED_BY_ACTIVE_LEASE`, `GC_BLOCKED_BY_RECOVERY`, exit 5). Execution recomputes the census inside an immediate write transaction, records a `gc_runs` row and `gc_tombstones`, moves files to a private trash inside that transaction, unlinks after commit and finishes interrupted runs first. Provider cache entries older than `cache.unreferenced_retention_days`, incomplete entries past the grace, and the oldest entries beyond `cache.max_size_mb` are deleted metadata-first. Anki media, backups, resources, journals and snapshots are never candidates.

## OP-57 — `resources list`

Inputs: Installed/required/resource filters.

Effects: Local read.

1. Read manifests/hashes/licenses and selected settings requirements.
2. Report missing engines/packs/models/voices; optional remote catalogue requires explicit fetch and offline checks.

Result/failure: No download or automatic model selection. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current behavior (WP-14): `resources list [--installed|--required] [--resource PREFIX]` re-verifies each receipt under `storage.resource_dir/receipts` (verified, modified, missing_files), lists unreceipted tessdata files, reports `config validate` local resource checks, required Tesseract packs (`ocr.languages` plus purpose OCR mappings) and the configured Ollama model as not checked. No catalogue is fetched.

## OP-58 — `resources install RESOURCE`

Inputs: Explicit pinned source/version/checksum/license; destination.

Effects: Explicit local/network resource installation, no Anki write.

1. Validate destination/source allowlist/offline policy/license and free space.
2. Download to temp under byte/deadline limits; verify checksum/type/archive paths.
3. Install atomically; no shell install scripts or overwrite shared user resources.
4. For Ollama model use explicit supported pull with digest receipt; never pull on generation fallback.

Result/failure: Resource receipt; unverifiable artifacts stay unavailable. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

Current behavior (WP-14): `resources install KIND:NAME --source SRC --version V --sha256 HEX --license SPDX [--destination DIR] [--execute]` previews by default. Kinds: `tesseract` (one `NAME.traineddata` linked into the shared `tessdata` directory), `piper` and `file` (versioned directories), `ollama` (`--source ollama-registry`). Sources are absolute paths, `file://`, or HTTP(S) on loopback, builtin resource hosts or `network.allowed_remote_service_hosts`; credentials are refused; offline mode blocks downloads and pulls. Downloads stream to a private staging directory under the resource dir with `resources.max_download_mb`, redirect host checks and a deadline of 60 × `network.request_timeout_seconds`. The SHA-256 must match before anything is installed. Zip archives (stored/deflate) are checked entry by entry before extraction (no absolute or `..` paths, symlinks, special files, encryption, duplicates or file/directory conflicts; `resources.max_unpacked_mb` counted on actual bytes). Files are written 0600 and never executed. Existing destinations are never overwritten; an identical receipt is an idempotent no-op. A JSON receipt records source, final URL, version, license, digest and per-file hashes. Ollama installs POST `/api/pull` to the loopback `llm.endpoint` and compare `/api/tags` digests; a mismatch writes no receipt and leaves the pulled model in Ollama.

## OP-59 — `recover inspect`

Inputs: Operation ID or --pending.

Effects: Local read; explicit --live Anki read.

1. Enumerate journal unknown/partial states, snapshots and intended effects.
2. With --live collect current evidence without mutation.
3. Report safe actions, ambiguity and exact reconciliation/restore command.

Result/failure: Inspection never retries writes. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-60 — `recover reconcile OPERATION`

Inputs: Operation ID; --apply required for remaining effects; --rebind explicitly authorizes verified session-epoch continuation.

Effects: Read-only proposal or journaled recovery writes.

1. Load same operation/settings/identity and run ALG-RECONCILE inspection.
2. Without --apply show matched/absent/partial/conflict evidence.
3. With --apply continue only proven-safe effects; ambiguity requires observed-state-bound review.

Result/failure: Same operation receipt or needs_recovery, never fresh creation ID. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-61 — `completions SHELL`

Inputs: Supported shell name.

Effects: Stdout only.

1. Build from clap command schema without reading config/state or services.
2. Print completion script; do not modify shell startup files.

Result/failure: Script text; unknown shell usage error. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.
