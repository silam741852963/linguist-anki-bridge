# Doctor operations

Read [shared command rules](README.md) before implementing any handler.

## OP-01 — `doctor`

Inputs: Optional purpose/profile and --offline; no --apply. `--local` performs no service requests. `--ollama` probes only configured local Ollama model metadata and conflicts with `--local`. With no service selector, the current implementation probes Anki; combined comprehensive health reporting remains pending.

Effects: Read-only local/service probes.

1. Resolve ALG-CONFIG without writing config.
2. Check paths/free space/executable versions/resources and selected provider model capabilities.
3. Read Anki capability/model/profile manifests if reachable; distinguish read, add, mapped migration, backup and restore readiness.
4. Report each failure with a corrective command; do not install packages, start services or call mutation endpoints.

Result/failure: Health JSON/text with capabilities; unavailable optional capability is warning, required capability yields dependency exit. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.


Implemented Ollama mode: resolve and validate settings without creating state, construct the bounded local metadata client, read tags/show/tags, and return the verified metadata summary. Offline mode permits loopback probes. Exit 0 means the metadata probe succeeded, never generation/apply readiness. Missing selection/capability returns dependency exit; transport/schema failures use shared error handling. The summary omits raw provider bodies and credential values and reports `raw_assets_persisted=false`, `generation_ready=false`, collection writes disabled and release gates not run. No model load/pull/generation, Anki probe or asset publication occurs. Full path/resource checks and actionable corrective commands remain pending.
