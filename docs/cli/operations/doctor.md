# Doctor operations

Read [shared command rules](README.md) before implementing any handler.

## OP-01 — `doctor`

Inputs: Optional purpose/profile and --offline; no --apply. `--local` performs no service requests. `--ollama` probes only configured local Ollama model metadata. `--bridge` inspects the configured native companion declaration. These three selectors conflict. With no service selector, the current implementation probes Anki; combined comprehensive health reporting remains pending.

Effects: Read-only local/service probes.

1. Resolve ALG-CONFIG without writing config.
2. Check paths/free space/executable versions/resources and selected provider model capabilities.
3. Read Anki capability/model/profile manifests if reachable; distinguish read, add, mapped migration, backup and restore readiness.
4. Report each failure with a corrective command; do not install packages, start services or call mutation endpoints.

Result/failure: Health JSON/text with capabilities; unavailable optional capability is warning, required capability yields dependency exit. The shared wrapper supplies typed errors and leaves durable evidence for any started effect.


Implemented Ollama mode: resolve and validate settings without creating state, construct the bounded local metadata client, read tags/show/tags, and return the verified metadata summary. Offline mode permits loopback probes. Exit 0 means the metadata probe succeeded, never generation/apply readiness. Missing selection/capability returns dependency exit; transport/schema failures use shared error handling. The summary omits raw provider bodies and credential values and reports `raw_assets_persisted=false`, `generation_ready=false`, collection writes disabled and release gates not run. No model load/pull/generation, Anki probe or asset publication occurs. Full path/resource checks and actionable corrective commands remain pending.

Implemented bridge mode: `doctor --bridge [--offline]` makes only profile-pinned `getActiveProfile` and `labCapabilities` reads. It validates the strict `lab-native-v1` declaration and prints its manifest digest and declared session/actions/variants. A successful probe establishes only that the declaration was read and parsed; `compatibility_verified`, `collection_identity_verified`, and `collection_writes_enabled` remain false, and release gates remain `not_run`. A missing or malformed companion fails through the read-port error contract. No local state is created and no bridge control or mutation action is called.

Implemented local mode: `doctor --local` resolves settings and reports configured local prompt/schema/voice/OCR paths and browser/Piper/Tesseract helper candidates without running executables or contacting services. It also compares `storage.free_space_reserve_mb` with available bytes on the nearest existing directory above the selected state path, without creating that path. A symlinked or invalid path, an unavailable measurement, or insufficient space is a required gap. The measurement is a preflight observation, not a guarantee about later writes or future mount changes. The Tesseract candidate is required when that OCR engine is selected. The report marks each gap as required for the selected provider or optional; required gaps exit 3, optional gaps remain warnings with exit 0. It does not create state, verify OCR language packs, hashes/licenses/versions/models, or imply native write readiness.
