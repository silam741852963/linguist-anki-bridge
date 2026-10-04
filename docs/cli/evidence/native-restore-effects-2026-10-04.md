# Disposable native restore effects

On 2026-10-04, installed Anki reported version `25.09.2`, build `3d813c83`.
These commands passed:

```sh
cargo build --locked -q -p linguist-cli
./target/debug/linguist-anki-bridge --output json models builtin | /usr/bin/python3.14 scripts/verify-native-restore.py
./target/debug/linguist-anki-bridge --output json models builtin | /usr/bin/python3.14 scripts/verify-native-apply.py
python3 -m unittest discover -s addons/linguist_bridge/tests
```

The script creates a collection in a temporary directory and installs the CLI's
exact managed vocabulary manifest. It calls the unregistered companion helpers in
`addons/linguist_bridge/effects.py` directly on the open collection. It never
opens a user profile. The WP-11 apply probe was rerun because `update_note` now
shares the mapped note-type helper with `restore_note`.

1. Restore after later study. A studied managed note is updated (Production
   enabled, tag added, moved to the target deck), and its original card is
   studied again. `restore_note` sets the original fields and tags, moves the
   original card back to its home deck and leaves the new Production card where
   it is. The original card keeps its ID, every scheduler field and the
   review-log digest and count of the later study (two reviews). The new card
   keeps its ID and zero reviews.
2. Stale precondition. After a capture, a user edit makes `restore_note` fail
   `BRIDGE_PRECONDITION_FAILED`, and the user's value remains.
3. Reverse mapped note-type change. A studied Basic note is migrated to the
   managed model with Production enabled, which adds a new card, and the
   retained card is studied again. `restore_note` without the new card in
   `removed_card_ids` fails `BRIDGE_REVERSE_MAPPING_MISMATCH` and the note is
   unchanged. With the card listed, the note returns to Basic with the original
   fields. Only the retained card remains, with its ID, ordinal 0, home deck,
   scheduling and two-review history from after the conversion. The unstudied
   new-task card is removed by the note-type change.
4. Studied new-task card. When the card added by a migration has a review,
   `restore_note` fails `BRIDGE_STUDIED_CARD_REMOVAL`, and the note is unchanged.
5. Filtered decks. A card pulled into a filtered deck makes `restore_note` fail
   `BRIDGE_FILTERED_DECK`.
6. Created notes. Two notes are created with markers and a shared media file.
   `delete_unstudied_created_note` removes the unstudied one; a repeat fails
   `BRIDGE_NOTE_MISSING`. The studied one fails `BRIDGE_STUDIED_NOTE` and stays.
7. Shared state. The Basic and managed note types still exist, and the media file
   referenced by the deleted note keeps its bytes.

Scope of this proof: the reverse effect semantics on one Anki build in one
disposable collection, vocabulary notes only. It does not exercise AnkiConnect
registration, the operation ledger, owner/fence checks, the main-thread critical
section, serialization against UI edits, the Rust mutation transport, crash
windows, undo, or atomicity across the separate `change_notetype_of_notes`,
`update_note` and `set_deck` calls. It does not exercise grammar notes, split
groups, media restoration through `store_media` with alternate names, or Anki's
card generation when restored fields re-enable a template. The model manifest
digest is the WP-11 stand-in function.
