# Maintenance operations

Read [shared command rules](README.md) before implementing any handler.

## OP-55 — `cache status`

Inputs: Optional provider/purpose.

Effects: Local read.

1. Compute cache totals, reachable/protected/unreferenced bytes and retention policy.

Result/failure: No asset deletion. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-56 — `cache prune`

Inputs: Optional age/budget; --execute to delete local candidates.

Effects: Preview/local unreferenced cache deletion.

1. Run ALG-GC reachability and candidate preview.
2. With --execute lock/recheck and remove only safe unreferenced cache.

Result/failure: Prune receipt; no Anki media/history deletion. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-57 — `resources list`

Inputs: Installed/required/resource filters.

Effects: Local read.

1. Read manifests/hashes/licenses and selected settings requirements.
2. Report missing engines/packs/models/voices; optional remote catalogue requires explicit fetch and offline checks.

Result/failure: No download or automatic model selection. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

## OP-58 — `resources install RESOURCE`

Inputs: Explicit pinned source/version/checksum/license; destination.

Effects: Explicit local/network resource installation, no Anki write.

1. Validate destination/source allowlist/offline policy/license and free space.
2. Download to temp under byte/deadline limits; verify checksum/type/archive paths.
3. Install atomically; no shell install scripts or overwrite shared user resources.
4. For Ollama model use explicit supported pull with digest receipt; never pull on generation fallback.

Result/failure: Resource receipt; unverifiable artifacts stay unavailable. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.

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
