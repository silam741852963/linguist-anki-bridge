# Final native bridge decision

Owner: WP-03, WP-10–WP-13. Design settled; native correctness still requires disposable tests.

## Transport and packaging

Use a separate `addons/linguist_bridge/` companion installed explicitly into Anki. It adds namespaced actions to the **existing** AnkiConnect listener; it starts no server. The installed source was inspected read-only: module exports `ac`, class `AnkiConnect`, and `util.api`; dispatcher discovers decorated bound methods. The selected registration adapter adds unique class methods using that decorator after add-ons load, without replacing the handler, standard actions, constructor or HTTP listener. This is a pinned compatibility mechanism inferred from installed code, **not an upstream extension API guarantee**. Missing symbols, collisions, unexpected dispatcher semantics or untested source versions disable registration and managed writes. No silent runtime patch of installed files.

Build the companion as its own versioned `.ankiaddon` artifact with source/license notices; do not bundle Anki/AnkiConnect source into the MIT CLI. AnkiConnect-derived integration code carries its applicable GPL notices. Installation/activation is explicit; `doctor` never installs it. Compatibility matrix pins Anki build/API, AnkiConnect integration manifest and companion protocol. Unknown versions allow preparation/read-only inspection until verified.

All managed collection mutations require this bridge, including new notes and same-model updates. Plain AnkiConnect is sufficient for reads/preparation only; no weaker write fallback. The extra dependency resolves otherwise unavoidable race/idempotency ambiguity consistently across all four workflows.

## Protocol `lab-native-v1`

| Action | Purpose | Effects |
| --- | --- | --- |
| labCapabilities | Protocol/build/capability/collection-session manifest | Read-only |
| labBegin | Bind collection, approved digest and temporary app writer token | Sidecar/session only |
| labInspect | Consistent note/card/history/model/deck/media evidence | Read-only |
| labMutate | Submit one typed, bounded journaled effect | Async serialized native collection operation |
| labOperationStatus | queued/running/verified/failed/unknown receipt | Read-only |
| labRebind | Explicit manifest-bound continuation after session change | Sidecar authorization; no note mutation |
| labEnd | Release owner token after durable accounting | Sidecar/session only |

Typed labMutate variants: install_model, export_checkpoint, store_media, create_note, update_note, restore_note, delete_unstudied_created_note. No arbitrary SQL, code, commands, URLs, sync or import variant. The server validates version, operation UUID, payload digest, expected collection binding, pre-state, monotonic owner/fencing token and exact variant schema. It rejects unknown properties and digest reuse with different payload. Configured AnkiConnect API-key authentication is required for bridge mutation/control actions; reads work with the endpoint's existing policy. Doctor reports missing key configuration; no security bypass setting.

Implemented state (WP-17, 2026-10-06): every variant has an exact envelope `{schema_version:1, variant, body:{...}}`, byte-hashed as SHA-256 by both sides, with bounded typed bodies (`addons/linguist_bridge/payloads.py`); unknown properties, variants and replayed UUIDs with a different payload are refused. The `create_note` body keeps the shape above (`model_name`, `model_manifest_digest`, `deck_id`, `fields`, `tags`, `marker_tag`, `source_plan_digest`, `checkpoint_digest`, `binding`, `expected_absent`). Each variant accepts one approval kind: a reviewed plan digest for forward effects, a `lab-restore-decision-v1` digest for reverse effects, `checkpoint` and `model-install` digests for their operations. Model digests use the canonical manifest projection (`linguist_core::model::ManifestProjection`), the same in capture, apply and the companion. The companion re-checks model, deck, absence and source preconditions inside the critical section; the CLI journals intent before dispatch and accepts success only from its own read-back.

Baseline managed writes require same-host loopback and verified artifact accessibility; remote reads may be configured, remote mutation is deferred. Export/media paths are create-new beneath approved canonical storage roots; native store_media consumes an approved staged file reference plus hash/size instead of an unbounded base64 request; reject traversal/symlink escape or arbitrary read/write paths.

Network request returns an accepted operation ID promptly; CLI polls labOperationStatus under configured deadlines. Long reads/exports use serialized Anki QueryOp/CollectionOp; UI access stays on the main thread. A bounded single-note mutation obtains collection-operation serialization, then performs the source precondition check and synchronous native calls in one brief non-reentrant main-thread critical callback. It does not process Qt events, await network calls or display dialogs inside that callback; normal UI/AnkiConnect timer writes cannot interleave there. Schema confirmation happens beforehand, followed by a fresh precondition check. Do not run a whole-deck migration in one blocking callback. If an installed native call cannot satisfy this boundary, its write variant remains unavailable rather than pretending two separate calls are CAS. Native backend transaction/undo boundaries are tested and journaled; they are not assumed to span media/export/sidecar writes. [Anki's official operation guidance](https://addon-docs.ankiweb.net/background-ops.html) establishes serialization and undoable collection-operation patterns.

## Identity and exactly-once limits

Binding includes bridge installation UUID, sidecar lineage UUID, canonical profile/path fingerprint, and a fresh collection-session epoch on every load/import/restore. Epoch is execution identity; stable lineage/source manifests are approval identity. A copied UUID/profile alone is not a trustworthy collection incarnation. Hooks plus backend/path identity detect session replacement; source/post-state comparisons remain mandatory.

The read-only companion computes profile/path fingerprints as lowercase SHA-256 over UTF-8 `lab-profile-v1\0` + exact Anki profile name and `lab-path-v1\0` + canonical absolute collection path, respectively. The Rust read port compares the reported profile fingerprint with its surrounding `getActiveProfile` reads. It cannot verify the path from the HTTP endpoint, and neither hash proves collection contents or authorizes writes.

Session change stops dispatch. `--rebind` requires current --apply, matching sidecar lineage, trustworthy current manifests and a recorded ResumeBindingDecision; it changes execution binding without regenerating approved content. Unknown replacement/lost lineage remains needs_recovery. Native collection restore can replace contents without a unique external identity; never claim immunity to all replacement scenarios.

Bridge sidecar uses durable unique `(lineage, operation_uuid)` rows: intent/queued before acceptance; running before native effect; actual read-back before verified. Duplicate UUID+digest returns the existing state/receipt, including queued work, rather than dispatching again. A pending/unknown row after crash is reconciled, not blindly replayed. Owner expiry cannot authorize a new write while prior queued/running work is unresolved; stale fenced requests must not execute after owner replacement. Reconciliation may take ownership only with worker-liveness and state evidence. Server and collection commits are **not** one distributed transaction. A missing marker or lost sidecar cannot prove no write.

CLI ledger remains the source of immutable snapshots/intended effects; server ledger supplies dispatch deduplication and native receipts. For uncertain accepted creation, exact marker/fields/IDs/history evidence can adopt one result. No candidate, mismatched candidates or damaged lineage blocks until explicit observed-state-bound recovery. “Exactly once” means verified semantic outcome where evidence permits it, not a transport guarantee.

## Failure gate

Test delayed duplicate requests, restart after each ledger/native boundary, copied/removed markers, profile switch, same-profile import, lost sidecar, UI edits/reviews and source CAS conflicts. Scheduler-only reviews before the native critical section are permitted: capture current scheduler/history into fsynced native pre-effect evidence, preserve it through mutation and attach that supplementary evidence to the receipt without replacing the original immutable source snapshot. Content/model/task/deck drift still conflicts. Later append-only study after a verified native receipt is preserved and distinguished from migration-induced changes; ambiguous history changes enter recovery. If serialization or retained-card preservation fails, the affected mutation variant stays unavailable. No bridge is installed or exercised on the user's collection by this planning task.

A disposable Anki 25.09.2 probe found that a direct grammar-to-Basic note-type change deletes a studied Application child card while leaving its review row orphaned. Reverse planning must inspect every disappearing task's current card/history and reject this path for studied children until an alternative is proven. Retaining raw review rows without the card does not meet the history-preservation gate.

## Capability declaration wire shape

The Rust read port accepts a strict object with `protocol` (`lab-native-v1`), bounded `companion_version`, non-nil `bridge_id`, `integration` (`anki_version`, lowercase 64-character `anki_connect_source_digest`), nullable `collection_session`, unique known `actions`, unique typed `mutation_variants`, and `api_key_configured`. A session carries non-nil `lineage_id`/`session_epoch` plus lowercase SHA-256 profile/path fingerprints. Mutation declarations require all seven protocol actions, API-key configuration and a session. Unknown properties/actions/variants fail. `labCapabilities` is read-only and profile-pinned before/after its request. No self-reported verified flag is accepted: writes are enabled only by `Client::native_verified`, which also requires a loopback endpoint, a configured `anki.api_key_env` and a companion/Anki/AnkiConnect triple in the CLI's own pinned `VERIFIED_COMPANIONS` matrix (companion 0.1.0, Anki 25.09.2, the pinned AnkiConnect digest), proven by the disposable desktop scenarios.
