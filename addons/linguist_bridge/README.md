# Native companion (`lab-native-v1`)

This package is the Python companion add-on for the CLI's `lab-native-v1`
contract. The Rust CLI does not require Python at runtime; managed collection
writes require this add-on inside the Anki process, next to AnkiConnect.

Build the installable artifact from the repository root and install it through
Anki's add-on manager (Tools > Add-ons > Install from file):

```sh
python3 -m unittest discover -s addons/linguist_bridge/tests -v
/usr/bin/python3.14 -m unittest discover -s addons/linguist_bridge/tests -v  # with Anki's library
mkdir -p dist
python3 addons/build_addon.py "$(pwd)/dist/linguist-bridge.ankiaddon"
```

## Startup and registration

The entrypoint hooks `main_window_did_init`. It activates only when the Anki
build and the loaded AnkiConnect `__init__.py`/`util.py` bytes match the pinned
matrix in `compatibility.py` (Anki `25.09.2`, build `3d813c83`). Registration
is additive: it adds `lab*` methods with AnkiConnect's own `util.api`
decorator, checks `apiReflect` before and after, and never replaces the handler,
standard actions or the HTTP listener. Unknown builds, changed sources or name
collisions register nothing (or, for read-only builds, only the two read
actions). Anki itself always starts.

State lives under the Anki base folder in `linguist-anki-bridge-native/`
(private `0700` directory): installation identity, the lineage sidecar, the
operation ledger, a `staging/` directory for media handed over by the CLI and
an `exports/` directory for checkpoint packages.

## Actions

| Action | Effect |
| --- | --- |
| `labCapabilities` | Protocol, build, AnkiConnect digest, actions, session and the seven variants (only with an API key and a live session) |
| `labOperationStatus` | Ledger state and receipt for one operation UUID (`absent` when unknown) |
| `labBegin` | Checks the binding against the live session and issues an owner token with the next fence |
| `labEnd` | Releases the owner token |
| `labInspect` | Note, tagged notes, models, deck, media hash/bytes, checkpoint scope, full note evidence; tied to the session epoch |
| `labMutate` | Queues one typed variant; returns promptly |
| `labRebind` | Reports the current session for an explicit continuation; no mutation |

Controls require a configured AnkiConnect API key; the listener itself rejects
a request whose key differs. Mutation variants: `install_model`,
`export_checkpoint`, `store_media`, `create_note`, `update_note`,
`restore_note`, `delete_unstudied_created_note`. Each has an exact bounded
body (`payloads.py`); unknown properties, variants or replayed UUIDs with a
different payload are refused.

## Execution

`labMutate` validates the session epoch and the current owner fence, records
the intent as `queued` and returns. A worker then takes the collection
executor slot (Anki serializes collection operations through one worker) and
runs one brief critical callback on the main thread: owner and session are
checked again, the variant's read-only preflight runs, `running` is fsynced,
the native calls run, and an actual read-back closes the row as `verified`
with a receipt. A refusal before `running` is `failed_before_write` with the
refusal code; any failure after `running` is `unknown`, never success.
Checkpoint export runs in the collection worker like Anki's own exporter; the
session epoch survives only that companion-owned close/reopen, and only when
profile, path, file identity and collection handle are unchanged.

A new owner classifies rows left by a dead worker: `running` becomes
`unknown(worker_crash)`, `queued` becomes
`failed_before_write(worker_lost_before_write)`. Live in-flight work blocks a
new owner. Duplicate UUIDs with the same payload return the stored state and
receipt and never dispatch again. Server and collection commits are not one
transaction; the CLI reconciles uncertain outcomes from read-back evidence.

`LINGUIST_BRIDGE_FAULT_FILE` names a one-shot fault file used only by the
disposable fault-injection scenarios (`crash`, `disk_full`, `sleep` at
`before_running`, `after_effect` or `before_export`). Without that variable
the injector is inert.

## Licensing

Files in this directory are licensed under GPL-3.0-or-later as indicated in
their SPDX headers; the artifact carries the license text. No Anki or
AnkiConnect source is bundled.
