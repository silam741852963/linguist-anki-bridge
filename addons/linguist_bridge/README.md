# Native companion (in development)

This package contains the independent Python companion source for the CLI's
`lab-native-v1` contract. The Rust CLI does not require Python at runtime.

The current package imports no Anki/Qt modules, registers no actions and creates
no server, sidecar or installation/session identity. `protocol.build_capabilities`
requires a supplied durable installation UUID and pinned integration metadata;
it declares only a read-only capability action and no native session/effects.
Its manifest is a declaration, not verification of installed compatibility.

Run protocol tests from the repository root:

```sh
python3 -m unittest discover -s addons/linguist_bridge/tests -v
cargo test --locked -p linguist-anki --test read_port native
```

Python and Rust tests use the same capability fixture in `contracts/v2/fixtures/`.
The pinned additive registration adapter is tested with isolated fake modules.
Its source-pin matrix/startup activation and real Anki integration remain pending.
Explicit installation identity initialization is implemented with private files,
create-new publication and restart/concurrent-initializer tests. It is not
called by startup yet and does not establish collection identity.
A main-thread session tracker is tested with explicit lifecycle events and file/
backend replacement observations; actual Anki lifecycle hooks remain pending.
Explicit lineage metadata initialization now uses a private SQLite sidecar with
FULL synchronization, WAL and a bounded cross-process startup lock. Tests cover
concurrent creation, restart, lock timeout and corrupt/unsafe metadata rejection.
Path associations alone do not prove collection incarnation or authorize writes.
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
Initialization is explicit and not called by startup. The ledger cannot mark
success, dispatch Anki calls, reconcile unknown effects or verify native state.
`failed_before_write` records only that this inactive ledger never advanced the
request to running; it is not a verified no-effect receipt from Anki.
Variant-specific mutation bodies are not validated or dispatched. It is not yet
linked to collection lineage/session lifecycle or the Rust CLI
journal, so it grants no mutation capability. Serialized inspection,
authenticated controls, terminal receipts, packaging and native recovery tests
remain pending. Do not install this scaffold as a functioning bridge; no
installable artifact is produced yet.

Files in this directory are licensed under GPL-3.0-or-later as indicated in their
SPDX headers. The complete license text is available at
https://www.gnu.org/licenses/gpl-3.0.txt and must accompany the future artifact.
No Anki or AnkiConnect program source is bundled in this package.
