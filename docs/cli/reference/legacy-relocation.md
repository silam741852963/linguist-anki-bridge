# Legacy relocation record

Date: 2026-09-26. Existing implementation moved to `legacy/`; new CLI documents consolidated under `docs/cli/`. Git history/metadata, `.agents/`, `.codex/`, root license and repository README remain at root.

Moved together: Rust workspace/manifests/lockfile/crates; Python package/project metadata; tests; v1 contracts/fixtures; scripts; Arch packaging/desktop file; historical documentation; personal local mapping file; existing virtual environment/build/cache/temp directories. Original source edits and untracked implementation files were preserved. Personal mapping and artifacts remain ignored at their new paths.

The relocation checked SHA-256 equality for all 137 tracked/nonignored implementation files selected before moving, including existing modifications. Packaging recipes received only the required clone-build-root change to `legacy/`; README/document links and root ignore path were updated. Source logic was not rewritten.

Checks passed: relocated Rust workspace metadata resolves offline; 22 Python contract tests pass from `legacy/` with explicit PYTHONPATH; packaging/verification shell syntax valid; grouped settings/defaults, ID routes and Markdown links/anchors valid; tracked diff whitespace check valid. This was a relocation check, not a full application release/native migration test.

Moved virtual environments/editable installations and compiler artifacts may retain old absolute paths. Use the archived interpreter with `PYTHONPATH=src` for tests, recreate environments before using installed console scripts, and rebuild binaries when needed. Existing runtime artifacts were not regenerated or deleted. No Anki writes occurred.

Future root code may be added by the implementation work packages. `legacy/` remains the reference source; copying/adapting selected logic is distinct from treating its existing safety behavior as the new contract.
