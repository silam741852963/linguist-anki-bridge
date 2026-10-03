# Disposable collection-package restore

On 2026-10-03, installed Anki reported version `25.09.2`, build `3d813c83`.
This command passed:

```sh
/usr/bin/python3.14 scripts/verify-native-package-restore.py
cargo build --locked -q -p linguist-cli
/usr/bin/python3.14 scripts/verify-native-package-restore.py --cli target/debug/linguist-anki-bridge
```

The script creates a collection in a temporary directory with one Basic note,
one card reviewed once with Good, and a `voice.ogg` media file containing fixed
disposable bytes. It exports a current `.colpkg` with media, then calls the
installed Anki backend's collection-package importer into a separate temporary
collection path. Reopening the restored collection confirms the original note
and card IDs, field values, model/deck identity, template ordinal, scheduler
fields, memory-state field and complete review-log row. The
restored media file is byte-for-byte equal to the original.

The second invocation also runs the CLI's `backup inspect` command on that
same package, using a temporary config and scratch directory. The inspector
reports one note, one card, one review row and one declared media file of the
expected byte count. Container/media checks, SQLite integrity and Anki core
schema checks pass. It correctly leaves `checkpoint_eligible=false` because
the package has no source-matched checkpoint binding.

This is a successful full-package round trip for one small disposable fixture.
It does not certify a source-matched CLI checkpoint, native companion
serialization, recovery from a crash, large collections, shared media, a
profile import hook, or restoration after later user edits. The probe uses a
pinned Anki backend API directly; no user profile or live collection was opened.
