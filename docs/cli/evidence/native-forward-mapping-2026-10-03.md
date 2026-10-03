# Disposable forward mapping probe

On 2026-10-03, installed Anki reported version `25.09.2`, build `3d813c83`.
The following command passed:

```sh
cargo run --locked -q -p linguist-cli -- --output json models builtin | /usr/bin/python3.14 scripts/verify-native-forward-mapping.py
```

The script creates a collection under a temporary directory, installs the CLI's
exact managed grammar manifest, and creates one note in Anki's built-in Basic
note type. It answers the Basic card once with Good, then uses Anki's
`change_notetype_of_notes` API to map Front to Pattern and RecognitionPrompt,
Back to Meaning, and Basic template ordinal 0 to managed Recognition ordinal 0.

The script compares the original card ID, note ID, template ordinal, deck IDs,
queue/type, due date, interval, ease factor, repetition/lapse counts, FSRS memory
state and complete review-log rows before and after the conversion. All match.
It then enables Application with a complete exercise and confirms that Anki
creates a distinct ordinal-1 card with zero repetitions and no review log while
the original Recognition card's scheduling and review rows stay unchanged.
After another review of Recognition, converting the note back to Basic retains
the original card ID, updated scheduler fields and both review rows. The fresh,
unstudied Application card is removed by that one-template reverse mapping.

A separate disposable managed grammar note tests the dangerous case: its
Application card receives a Good review before the same reverse mapping.
Anki's native note-type change removes that studied card row, while its review
row remains in `revlog` under the removed card ID. A direct reverse mapping is
therefore unsafe for a studied child. The CLI must block that effect or use a
separately proven preservation route; orphaned review rows are not a retained
study card.

The same disposable collection also creates a three-card note in a synthetic
`2. Picture Words (disposable)` note type with the five observed field names and
three template ordinals. The test answers its Comprehension card once, maps old
ordinals 0/1/2 to managed vocabulary Comprehension/Production/Spelling 0/1/2,
and compares all three card IDs, scheduler fields and review-log rows before
and after. The synthetic source template content and field values are selected
to keep each managed target card eligible; they do not reproduce the installed
user note type or certify its actual template behavior.

This proves two specific forward mapping shapes in an isolated collection on
this installed Anki build. It does not prove the companion protocol, atomic CAS,
the real Picture Words source model, media export, general reverse mapping,
backup/restore or safety against concurrent UI edits. No user profile or live
collection was opened.
