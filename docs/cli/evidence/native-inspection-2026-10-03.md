# Disposable native inspection helper

On 2026-10-03, installed Anki reported version `25.09.2`, build `3d813c83`.
This command passed:

```sh
/usr/bin/python3.14 scripts/verify-native-inspection.py
```

The script creates one Basic note and card in a temporary collection, reviews
the card once and invokes the companion's inactive `inspect_note` helper. It
checks exact note/model IDs, field text, field/template order, card/deck IDs,
repetition count and the card's complete observed review row. A discovered
`[sound:voice.ogg]` file is read through a no-follow media-directory handle;
the returned byte length, SHA-256 and Base64 bytes match the source. A missing
reference is reported as missing, while a symlinked media file, noncanonical
Anki wire ID and oversized private field fail closed.

The helper now retains Anki's full returned model dictionary alongside the
normalized fields/templates. The disposable probe compares two complete reads
of note, model, cards, review rows and discovered media bytes. A focused unit
test changes a review row between the reads and confirms
`BRIDGE_INSPECT_SOURCE_DRIFT`; matching reads set
`repeated_reads_matched=true` while `atomic_snapshot_verified` stays false.

The helper is main-thread only and bounds note, card, review, discovered media
and serialized result size. Internal runtime inspection requires the current
canonical session epoch, checks the observed session before and after both reads,
and returns the bound session. A stale epoch or changed profile fails closed.
It makes no network request or collection mutation.
It reports the observed media bytes but keeps
`media_references_complete=false`, `atomic_snapshot_verified=false`,
`model_manifest_complete=false` and `write_authorized=false`. It is not
registered as `labInspect`, though its inactive code is in the read-only
`.ankiaddon` artifact: QueryOp serialization, complete media discovery, backend model
coverage and native read-back contract tests remain before that action can be
advertised. No user profile or live collection was opened.
