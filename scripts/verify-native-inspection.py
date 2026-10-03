#!/usr/bin/env python3
"""Exercise the inactive native read helper on a disposable Anki collection."""

import base64
import hashlib
from pathlib import Path
import sys
import tempfile

from anki.buildinfo import buildhash, version
from anki.collection import Collection

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "addons"))
from linguist_bridge.inspection import (
    MAX_RESULT_BYTES, InspectionError, inspect_note, inspect_note_twice)


def main():
    assert (version, buildhash) == ("25.09.2", "3d813c83"), (version, buildhash)
    with tempfile.TemporaryDirectory(prefix="lab-native-inspection-") as directory:
        col = Collection(str(Path(directory) / "collection.anki2"))
        try:
            media_bytes = b"OggS\x00disposable-inspection-media"
            name = col.media.write_data("voice.ogg", media_bytes)
            assert name == "voice.ogg"
            deck_id = col.decks.id("Disposable inspection")
            basic = col.models.by_name("Basic")
            note = col.new_note(basic)
            note["Front"] = "食べる"
            note["Back"] = "to eat [sound:voice.ogg]"
            col.add_note(note, deck_id)
            card_id = col.card_ids_of_note(note.id)[0]
            card = col.get_card(card_id)
            card.start_timer()
            col.sched.answerCard(card, 3)
            observed = inspect_note_twice(col, str(note.id))
            assert observed["schema_version"] == 1
            assert observed["note_id"] == str(note.id)
            assert observed["note"]["fields"] == {
                "Front": "食べる", "Back": "to eat [sound:voice.ogg]"}
            assert observed["model"]["name"] == "Basic"
            assert observed["model"]["fields"] == ["Front", "Back"]
            assert observed["model_payload"] == col.models.get(note.mid)
            assert len(observed["model"]["templates"]) == 1
            assert len(observed["cards"]) == 1
            assert observed["cards"][0]["id"] == str(card_id)
            assert observed["cards"][0]["deck_id"] == str(deck_id)
            assert observed["cards"][0]["repetitions"] == 1
            assert len(observed["cards"][0]["reviews"]) == 1
            assert observed["cards"][0]["reviews"][0]["card_id"] == card_id
            assert observed["review_rows_untruncated"]
            assert observed["repeated_reads_matched"]
            assert not observed["atomic_snapshot_verified"]
            assert not observed["media_references_complete"]
            assert observed["discovered_media_bytes_verified"]
            assert observed["media"] == [{
                "name": name, "fields": ["Back"], "missing": False,
                "size_bytes": len(media_bytes),
                "sha256": hashlib.sha256(media_bytes).hexdigest(),
                "bytes_base64": base64.b64encode(media_bytes).decode("ascii"),
            }]
            assert not observed["write_authorized"]
            for bad in ("0", "01", "+1", "9007199254740992"):
                try:
                    inspect_note(col, bad)
                except InspectionError as error:
                    assert str(error) == "BRIDGE_INSPECT_ID_INVALID"
                else:
                    raise AssertionError(("accepted invalid note ID", bad))
            note["Back"] = "[sound:missing.ogg]"
            col.update_note(note)
            missing = inspect_note(col, str(note.id))
            assert missing["media"][0]["missing"]
            assert not missing["discovered_media_bytes_verified"]
            external = Path(directory) / "external.ogg"
            external.write_bytes(b"outside-media")
            (Path(col.media.dir()) / "linked.ogg").symlink_to(external)
            note["Back"] = "[sound:linked.ogg]"
            col.update_note(note)
            try:
                inspect_note(col, str(note.id))
            except InspectionError as error:
                assert str(error) == "BRIDGE_INSPECT_MEDIA_FILE_INVALID"
            else:
                raise AssertionError("followed symlinked media")
            note["Back"] = "x" * (MAX_RESULT_BYTES + 1)
            col.update_note(note)
            try:
                inspect_note(col, str(note.id))
            except InspectionError as error:
                assert str(error) == "BRIDGE_INSPECT_RESULT_LIMIT"
            else:
                raise AssertionError("accepted oversized private field")
        finally:
            col.close()
    print("PASS: bounded native helper observes note/card/review and discovered media bytes")


if __name__ == "__main__":
    main()
