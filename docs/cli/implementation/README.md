# Implementation work packages

Status: finalized implementation sequence; evidence gates remain not run. No package is complete merely because these documents exist. Follow [handbook](../README.md), [operations](../operations/README.md) and referenced algorithms. Preserve unrelated working-tree changes. Do not implement against the user's live collection as a test target.

Path rule: existing code lives in `legacy/`. Unprefixed `crates/`, `contracts/`, `scripts/` and workspace paths in packages mean the future repository-root implementation; they do not instruct editing archived code. Inspect equivalent `legacy/` paths for reuse, then copy/adapt selected logic with compatibility tests. The root Cargo workspace now contains the new CLI and core library; see [implementation status](status.md).

## Architecture and boundaries

Create `crates/linguist-cli` for clap parsing/output/composition and port live use-case adapters from `legacy/crates/linguist-desktop/src/backend.rs` into new application/provider crates. CLI imports application/domain/provider/config/storage libraries, never desktop/QML. Keep archived Python under `legacy/` for parity/import tests and optional subprocess helpers while replacing runtime dependencies deliberately. No GUI/server is required to build/run CLI; workspace default members and feature flags must make Qt optional. Prompts/templates/schema resources belong in a versioned resource directory, not `include_str!` paths into a retired Python package.

Use Rust 1.98.1/edition 2024 and researched libraries with locked versions/minimal features; test MSRV and isolate dependency upgrades. SQLite stores indexed metadata/leases/journals with migrations and transactional writes; content-addressed files store large archives/assets. A database transaction does not make Anki mutations transactional. Provider ports expose typed rich evidence/errors/capabilities. A restricted native mapped-migration extension runs within Anki's existing add-on process; its selected additive registration/protocol is defined in [native decisions](../decisions/native-bridge.md); compatibility tests gate writes.

Implement pure domain algorithms and fake ports first, local commands second, read adapters third, and write adapters only after failure tests and disposable-native evidence. Never leave a partially implemented unsafe write fallback enabled.

## Work package index

| Package | Deliverable | Depends on | Operations |
| --- | --- | --- | --- |
| WP-01 | v2 types/resources and compatibility boundary | — | Shared contracts |
| WP-02 | CLI shell and typed config | WP-01 | OP-01–OP-10, OP-61 |
| WP-03 | Read capabilities and native safety feasibility | WP-01 | OP-11–OP-20 foundations |
| WP-04 | Durable state, hashes, archives, leases | WP-01, WP-02 | OP-25–OP-27, OP-35–OP-38, OP-48–OP-49 |
| WP-05 | Selection/source capture/mapping | WP-03, WP-04 | OP-11–OP-16, OP-18–OP-20 |
| WP-06 | Provider/OCR/resource boundaries | WP-02, WP-04 | OP-57–OP-58, provider consumers |
| WP-07 | Vocabulary preparation | WP-05, WP-06 | OP-21–OP-22 |
| WP-08 | Grammar preparation and split planning | WP-05, WP-06 | OP-23–OP-24 |
| WP-09 | Review/edit/validation/render/approval/export | WP-07, WP-08 | OP-28–OP-33, OP-51 |
| WP-10 | Checkpoints and model installation | WP-03, WP-04, WP-09 | OP-17, OP-52–OP-54 |
| WP-11 | Apply/mapped migration/reconciliation | WP-10 | OP-34, OP-59–OP-60 |
| WP-12 | Restore and split recovery | WP-11 | OP-44, OP-50 |
| WP-13 | Jobs and controls | WP-09, WP-11, WP-12 | OP-35–OP-47 |
| WP-14 | Imports/resources/cache and complete settings | WP-04, WP-06, WP-13 | OP-09–OP-10, OP-47, OP-55–OP-58 |
| WP-15 | Packaging, UX and four-workflow release checks | WP-01–WP-14 | Every operation |
| WP-16 | Independent safety/documentation audit | WP-15 | Every operation/settings/invariant |

Ranges above are ownership summaries; OP-51 export can ship before restore. WP-03 implements/verifies the selected bridge while CLI/state work advances. WP-14 resource installation core is needed by WP-06; WP-14 finishes migration/pruning rather than creating a circular dependency. WP-16 means a separate review pass, not mandated sub-agent delegation.

## Minimum release evidence table

| Requirement | Proof |
| --- | --- |
| All four workflows | Four end-to-end disposable collection scenarios |
| CLI only | Headless build/run/help, no desktop dependency |
| Every setting configurable | Registry/generated example + nondefault consumer coverage |
| Writes explicitly authorized | All mutation handlers reject missing current --apply |
| Recoverable unknown effects | Crash/timeout injection at each forward/reverse boundary |
| History preserved | Real native card-ID/scheduler/review-log assertions |
| Source preserved | Full field/media archival and no collection-media deletion |
| Restore after later study | Later review retained; user content drift conflict tested |
| Backup useful | Disposable package restore verifies claimed coverage |
| Honest usability | Actionable issues/next commands; structured output stable |

## Package files

- [WP-01 — explicit domain and resources](wp-01.md)
- [WP-02 — CLI/config shell](wp-02.md)
- [WP-03 — Anki feasibility gate](wp-03.md)
- [WP-04 — durable store](wp-04.md)
- [WP-05 — selectors and source capture](wp-05.md)
- [WP-06 — provider boundary](wp-06.md)
- [WP-07 — vocabulary](wp-07.md)
- [WP-08 — grammar](wp-08.md)
- [WP-09 — review and approval](wp-09.md)
- [WP-10 — checkpoints/model actions](wp-10.md)
- [WP-11 — apply/reconcile](wp-11.md)
- [WP-12 — restore/splits](wp-12.md)
- [WP-13 — job executor](wp-13.md)
- [WP-14 — migration/pruning/setting closure](wp-14.md)
- [WP-15 — release/UX](wp-15.md)
- [WP-16 — final review and traceability](wp-16.md)
