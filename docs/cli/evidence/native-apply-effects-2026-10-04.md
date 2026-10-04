# Disposable native apply effects

On 2026-10-04, installed Anki reported version `25.09.2`, build `3d813c83`.
These commands passed:

```sh
cargo build --locked -q -p linguist-cli
./target/debug/linguist-anki-bridge --output json models builtin | /usr/bin/python3.14 scripts/verify-native-apply.py
python3 -m unittest discover -s addons/linguist_bridge/tests
```

The script creates a collection in a temporary directory and installs the CLI's
exact managed vocabulary manifest. It calls the unregistered companion helpers in
`addons/linguist_bridge/effects.py` directly on the open collection. It never
opens a user profile.

1. `create_note` adds one managed note with the operation marker tag in the
   target deck. A `tag:` search for the marker returns exactly that note, whose
   single Comprehension card has zero reviews. A second create with the same
   marker fails `BRIDGE_PRECONDITION_FAILED`, and the search still returns one note.
2. A managed note in another deck is studied once and captured (the preparation
   view), then studied again (normal study between preparation and apply). The
   content precondition digest is the same for both captures. The fresh capture
   shows two reviews. `update_note` with that fresh precondition sets new
   fields, enables Production, adds one tag and moves the note to the target
   deck. The original card keeps its ID, scheduler fields (queue, type, due,
   interval, ease, repetitions, lapses, left, original due, flags, memory state),
   review-log digest and count. Anki adds one new Production card with zero
   reviews. Both cards end in the target deck, and both tags are present.
3. After a capture, a user edit to Meaning makes `update_note` fail
   `BRIDGE_PRECONDITION_FAILED`, and the user's value remains.
4. A studied Basic note is migrated to the managed vocabulary model with the
   explicit ordinal map 0 to 0. The note type changes, and the single card keeps
   its ID, scheduling and review history while it moves to the target deck.
5. After a filtered deck pulls that card, `update_note` fails
   `BRIDGE_FILTERED_DECK`. The script then empties the filtered deck.
6. `store_media` writes verified bytes under the exact name and accepts an
   identical repeat. Different bytes under the same name fail
   `BRIDGE_MEDIA_COLLISION`, and the original bytes remain.

The Anki-free Python tests check the same media rules, unsafe names and a
precondition-digest vector. A Rust test asserts the same vector, so the Rust
orchestration and the companion compute the identical precondition digest for
that input (non-ASCII text, quotes, newline, duplicate tags, unsorted cards).

Scope of this proof: the effect semantics on one Anki build in one disposable
collection. It does not exercise AnkiConnect registration, the operation ledger,
owner/fence checks, the main-thread critical section, serialization against UI
edits, the Rust mutation transport, crash windows, or atomicity across the
separate `change_notetype_of_notes`, `update_note` and `set_deck` calls. The
model manifest digest used here is a stand-in function. It is not the digest that
AnkiConnect read capture records.
