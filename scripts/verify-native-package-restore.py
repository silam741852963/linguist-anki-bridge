#!/usr/bin/env python3
"""Export and restore a disposable Anki collection with study and media bytes."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

from anki._backend import RustBackend
from anki.buildinfo import buildhash, version
from anki.collection import Collection
from anki.media import media_paths_from_col_path


def snapshot(col, note_id, card_id):
    note = col.get_note(note_id)
    card = col.get_card(card_id)
    reviews = col.db.all(
        "select id, cid, usn, ease, ivl, lastIvl, factor, time, type "
        "from revlog where cid = ? order by id", card_id)
    return {
        "note_id": note.id, "model_id": note.mid,
        "front": note["Front"], "back": note["Back"],
        "card_id": card.id, "ordinal": card.ord, "deck_id": card.did,
        "queue": card.queue, "type": card.type, "due": card.due,
        "interval": card.ivl, "ease_factor": card.factor,
        "repetitions": card.reps, "lapses": card.lapses,
        "memory_state": str(card.memory_state),
        "last_review_time": card.last_review_time,
        "reviews": reviews,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path,
                        help="optional built linguist-anki-bridge binary for package inspection")
    args = parser.parse_args()
    assert (version, buildhash) == ("25.09.2", "3d813c83"), (version, buildhash)
    media_bytes = b"OggS\x00disposable-voice-bytes\x00"
    with tempfile.TemporaryDirectory(prefix="lab-native-package-restore-") as directory:
        root = Path(directory)
        source_dir = root / "source"
        target_dir = root / "target"
        source_dir.mkdir()
        target_dir.mkdir()
        source_path = source_dir / "collection.anki2"
        target_path = target_dir / "collection.anki2"
        package = root / "checkpoint.colpkg"
        col = Collection(str(source_path))
        try:
            name = col.media.write_data("voice.ogg", media_bytes)
            assert name == "voice.ogg"
            deck_id = col.decks.id("Disposable restore test")
            basic = col.models.by_name("Basic")
            note = col.new_note(basic)
            note["Front"] = "食べる"
            note["Back"] = f"to eat [sound:{name}]"
            col.add_note(note, deck_id)
            card_id = col.card_ids_of_note(note.id)[0]
            card = col.get_card(card_id)
            card.start_timer()
            col.sched.answerCard(card, 3)
            before = snapshot(col, note.id, card_id)
            assert len(before["reviews"]) == 1
            assert (source_dir / "collection.media" / name).read_bytes() == media_bytes
            col.export_collection_package(str(package), include_media=True, legacy=False)
        finally:
            col.close()

        if args.cli is not None:
            scratch = root / "scratch"
            scratch.mkdir()
            environment = os.environ.copy()
            config = root / "config.toml"
            config.write_text("[config]\nversion = 2\n")
            environment["LAB_CONFIG"] = str(config)
            result = subprocess.run(
                [str(args.cli.resolve()), "--output", "json", "--set",
                 f"backup.verify_scratch_dir={scratch}",
                 "backup", "inspect", str(package)],
                env=environment, capture_output=True, text=True)
            assert result.returncode == 0, (result.stdout, result.stderr)
            inspection = json.loads(result.stdout)["inspection"]
            assert inspection["collection_note_count"] == 1
            assert inspection["collection_card_count"] == 1
            assert inspection["collection_review_count"] == 1
            assert inspection["declared_media_files"] == 1
            assert inspection["declared_media_bytes"] == len(media_bytes)
            assert inspection["container_and_declared_media_verified"]
            assert inspection["sqlite_integrity_verified"]
            assert inspection["anki_core_schema_verified"]
            assert not inspection["checkpoint_eligible"]
            print("PASS: CLI inspector verifies disposable package counts and media declaration")

        media_folder, media_db = media_paths_from_col_path(str(target_path))
        backend = RustBackend()
        backend.import_collection_package(
            col_path=str(target_path), backup_path=str(package),
            media_folder=media_folder, media_db=media_db)
        restored = Collection(str(target_path))
        try:
            after = snapshot(restored, note.id, card_id)
            assert after == before, (before, after)
            assert restored.card_ids_of_note(note.id) == [card_id]
            assert (Path(media_folder) / name).read_bytes() == media_bytes
        finally:
            restored.close()
    print("PASS: disposable colpkg restores note, reviewed card and exact media bytes")


if __name__ == "__main__":
    main()
