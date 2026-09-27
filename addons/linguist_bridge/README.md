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
Durable operation sidecar, session hooks,
serialized inspection, authenticated controls, mutations, packaging and native
recovery tests remain pending. Do not install this scaffold as a functioning
bridge; no installable artifact is produced yet.

Files in this directory are licensed under GPL-3.0-or-later as indicated in their
SPDX headers. The complete license text is available at
https://www.gnu.org/licenses/gpl-3.0.txt and must accompany the future artifact.
No Anki or AnkiConnect program source is bundled in this package.
