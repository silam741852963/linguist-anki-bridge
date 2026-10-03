# Disposable checkpoint scope and decode restoration

On 2026-10-04, installed Anki reported version `25.09.2`, build `3d813c83`.
These commands passed:

```sh
cargo build --locked -q -p linguist-cli
/usr/bin/python3.14 scripts/verify-native-checkpoint-scope.py --cli target/debug/linguist-anki-bridge
/usr/bin/python3.14 scripts/verify-native-package-restore.py --cli target/debug/linguist-anki-bridge
```

The first script creates a collection in a temporary directory. It adds one
Basic note whose back field references a `voice.ogg` media file with fixed
disposable bytes, and answers its card once with Good. From the open collection it builds a
checkpoint scope manifest: the note ID, the card ID with its repetitions and
review-log count, the note-type ID and the media file's SHA-1. The installed
Anki backend then exports two current `.colpkg` files, one with media and one
without, reopening the collection between exports. Finally it answers the card a
second time and builds a second manifest from that later state.

`backup verify FILE --scope-manifest SCOPE --restore-test-target DIR` on the
package with media reports one verified note, card, review, note type and media
file, with schema and scheduling tables included. The decode restoration test
writes the collection and media into a private directory below the target,
reopens them and finds the same scope report. It removes that directory and
reports `anki_importer_used=false` and `checkpoint_eligible=false` (the file is
unregistered). The package exported without media fails with
`CHECKPOINT_SCOPE_MEDIA_MISSING`. The earlier package checked against the later
manifest fails with `CHECKPOINT_SCOPE_SCHEDULING_MISSING`, so a stale checkpoint
cannot cover later study.

The second command repeats the 2026-10-03 [package restore](native-package-restore-2026-10-03.md)
probe after the inspector refactor; it still passes.

This proves scope verification and the decode restoration test against real
packages from one small disposable collection on this Anki build. It does not
exercise a native `export_checkpoint` adapter, a stored receipt created from a
live export, Anki's importer for the CLI's restore test, large collections,
shared media, concurrent UI edits or crash recovery. No user profile or live
collection was opened.
