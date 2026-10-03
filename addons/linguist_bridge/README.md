# Native companion (in development)

This package contains the independent Python companion source for the CLI's
`lab-native-v1` contract. The Rust CLI does not require Python at runtime.

The repository source package imports no Anki/Qt modules, registers no actions and
creates no server, sidecar or installation/session identity. The separately built
`.ankiaddon` entrypoint hooks Anki's main-window initialization; it creates
private identity, lineage and read-only ledger files under the Anki base directory
only after an exact build and AnkiConnect source match. It never touches the
collection. `protocol.build_capabilities`
requires a supplied durable installation UUID and pinned integration metadata;
it declares only registered read-only actions and no native effects. After an
observed collection load, the manifest may include a checked session identity;
that observation does not certify collection identity for writes.
Its manifest is a declaration, not verification of installed compatibility.

Run protocol tests or build an installable development artifact from the repository root:

```sh
python3 -m unittest discover -s addons/linguist_bridge/tests -v
cargo test --locked -p linguist-anki --test read_port native
mkdir -p dist
python3 addons/build_read_only_addon.py "$(pwd)/dist/linguist-bridge-read-only.ankiaddon"
```

Python and Rust tests use the same capability fixture in `contracts/v2/fixtures/`.
The pinned additive registration adapter can register read-only capabilities and
operation-status actions together. Isolated fake-module tests check unchanged
standard dispatch and all-or-nothing rollback. A caller must supply a bound
ledger status function and advertise the status action only after registration.
The packaged startup hook is implemented and isolated fake-hook tests cover
deferred activation, private identity persistence and unsupported-build refusal.
Real Anki integration remains pending.
The read-only matrix currently pins local Anki `25.09.2` (build `3d813c83`)
and the exact inspected AnkiConnect `__init__.py`/`util.py` bytes. A different
build or changed add-on source fails before registration. This is an isolated
compatibility guard, not a live integration or mutation certification; real Anki
activation and disposable Anki tests remain pending.
Explicit installation identity initialization is implemented with private files,
create-new publication and restart/concurrent-initializer tests. Pinned read-only
startup calls it, but it does not establish collection identity.
A main-thread session tracker now binds to the pinned build's collection load,
temporary close/reopen and profile-close hooks. It creates a fresh epoch on each
load/reopen, invalidates on close or observed file/handle replacement, and reports
the checked session through the read-only capability declaration. Hook behavior is
covered by isolated fixtures; real Anki lifecycle tests remain pending.
Explicit lineage metadata initialization now uses a private SQLite sidecar with
FULL synchronization, WAL and a bounded cross-process startup lock. Tests cover
concurrent creation, restart, lock timeout and corrupt/unsafe metadata rejection.
Pinned read-only startup opens and verifies this sidecar before registering its
actions; corrupt lineage metadata prevents registration. No collection lineage
is allocated at startup. Path associations alone do not prove collection
incarnation or authorize writes.
An inactive operation ledger now records bounded versioned request envelopes, exact
payload/approval/session/owner identities and append-only queued/running/unknown
events. A queued request may end as `failed_before_write` under the same owner and
fence, with an explicit reason; only then can a later request queue. The running
event must be durable before any future dispatcher calls Anki. Duplicate
UUID+identical payload replays the existing status; changed metadata conflicts.
Running/unknown states survive restart, block later requests and never authorize
automatic redispatch. Private file checks, FULL-sync SQLite, immutable-event
triggers, event hash links and concurrent transition tests cover this local
sidecar boundary.
Pinned read-only startup opens the ledger, but does not queue an operation. The ledger cannot mark
success, dispatch Anki calls, reconcile unknown effects or verify native state.
`failed_before_write` records only that this inactive ledger never advanced the
request to running; it is not a verified no-effect receipt from Anki.
The `create_note` body now has a strict managed-model intent schema, checked
again when reopening stored evidence. Other variants fail at queue time; none
is dispatched. The complete `lab-jcs-v1:plan:<hash>` approval digest is bound
to the payload and ledger status. This is structural validation only. It is not yet
linked to collection lineage/session lifecycle or the Rust CLI
journal, so it grants no mutation capability. Serialized collection inspection,
authenticated controls, terminal receipts, packaging and native recovery tests
remain pending. The generated artifact is a read-only development preview, not a
functioning mutation bridge; do not use it for managed collection writes.

An inactive `inspection.py` helper can read one bounded standard note, its model,
cards, review rows and discovered local media bytes on the main thread.
A [disposable Anki probe](../../docs/cli/evidence/native-inspection-2026-10-03.md)
exercises it. It retains Anki's returned model dictionary and compares two
bounded observations for drift. The runtime binds internal inspection to an
observed session epoch before and after those reads. The helper is packaged but
not registered as `labInspect`; it does not claim an atomic snapshot, complete
media discovery or write authority.

Files in this directory are licensed under GPL-3.0-or-later as indicated in their
SPDX headers. The complete license text is available at
https://www.gnu.org/licenses/gpl-3.0.txt and must accompany the future artifact.
No Anki or AnkiConnect program source is bundled in this package.
