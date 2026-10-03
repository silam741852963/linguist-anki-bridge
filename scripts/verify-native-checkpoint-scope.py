#!/usr/bin/env python3
"""Check CLI checkpoint scope verification and decode restoration against packages
exported by the installed Anki backend from a disposable collection."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from anki.buildinfo import buildhash, version
from anki.collection import Collection


def run(cli, root, *args):
    scratch = root / "scratch"
    scratch.mkdir(exist_ok=True)
    config = root / "config.toml"
    config.write_text("[config]\nversion = 2\n")
    environment = os.environ.copy()
    environment["LAB_CONFIG"] = str(config)
    return subprocess.run(
        [str(cli.resolve()), "--output", "json",
         "--set", f"backup.verify_scratch_dir={scratch}",
         "--set", f"storage.state_dir={root / 'state'}",
         "backup", "verify", *map(str, args)],
        env=environment, capture_output=True, text=True)


def manifest(col, note_id, media):
    cards = []
    for card_id in sorted(col.card_ids_of_note(note_id)):
        card = col.get_card(card_id)
        reviews = col.db.scalar("select count() from revlog where cid = ?", card_id)
        cards.append({"card_id": card_id, "note_id": note_id,
                      "reps": card.reps, "review_count": reviews})
    note = col.get_note(note_id)
    return {
        "schema_version": 1,
        "requirement": {"scheduling": True, "media": True, "schema": True},
        "note_ids": [note_id],
        "cards": cards,
        "model_ids": [note.mid],
        "media": [{"name": name, "sha1": hashlib.sha1(data).hexdigest()}
                  for name, data in sorted(media.items())],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True,
                        help="built linguist-anki-bridge binary")
    args = parser.parse_args()
    assert (version, buildhash) == ("25.09.2", "3d813c83"), (version, buildhash)
    media_bytes = b"OggS\x00disposable-checkpoint-voice\x00"
    with tempfile.TemporaryDirectory(prefix="lab-native-checkpoint-scope-") as directory:
        root = Path(directory)
        (root / "source").mkdir()
        (root / "restore").mkdir()
        with_media = root / "with-media.colpkg"
        without_media = root / "without-media.colpkg"
        path = str(root / "source" / "collection.anki2")
        col = Collection(path)
        try:
            name = col.media.write_data("voice.ogg", media_bytes)
            assert name == "voice.ogg"
            note = col.new_note(col.models.by_name("Basic"))
            note["Front"] = "食べる"
            note["Back"] = f"to eat [sound:{name}]"
            col.add_note(note, col.decks.id("Disposable checkpoint test"))
            card = col.get_card(col.card_ids_of_note(note.id)[0])
            card.start_timer()
            col.sched.answerCard(card, 3)
            scope = manifest(col, note.id, {name: media_bytes})
            # Collection-package export closes the collection.
            col.export_collection_package(str(with_media), include_media=True, legacy=False)
        finally:
            col.close()
        col = Collection(path)
        try:
            col.export_collection_package(str(without_media), include_media=False, legacy=False)
        finally:
            col.close()
        col = Collection(path)
        try:
            # Later study is not in the earlier package.
            card = col.get_card(card.id)
            card.start_timer()
            col.sched.answerCard(card, 3)
            later = manifest(col, note.id, {name: media_bytes})
        finally:
            col.close()
        scope_file = root / "scope.json"
        scope_file.write_text(json.dumps(scope))
        later_file = root / "later.json"
        later_file.write_text(json.dumps(later))

        result = run(args.cli, root, with_media, "--scope-manifest", scope_file,
                     "--restore-test-target", root / "restore")
        assert result.returncode == 0, (result.stdout, result.stderr)
        report = json.loads(result.stdout)
        assert report["inspection"]["collection_scope_verified"]
        assert report["scope_report"] == {
            "notes_verified": 1, "cards_verified": 1, "reviews_verified": 1,
            "models_verified": 1, "media_verified": 1,
            "schema_included": True, "scheduling_included": True,
        }, report["scope_report"]
        restoration = report["restoration"]
        assert restoration["passed"] and restoration["restored_media_verified"]
        assert restoration["restored_review_count"] == 1
        assert restoration["restored_media_files"] == 1
        assert restoration["scope"] == report["scope_report"]
        assert restoration["anki_importer_used"] is False
        assert report["checkpoint_eligible"] is False
        assert list((root / "restore").iterdir()) == []
        print("PASS: Anki-exported package covers note, card, review, note type and media; decode restore matches")

        result = run(args.cli, root, without_media, "--scope-manifest", scope_file)
        assert result.returncode != 0 and "CHECKPOINT_SCOPE_MEDIA_MISSING" in result.stderr, result
        print("PASS: package exported without media fails media scope")

        result = run(args.cli, root, with_media, "--scope-manifest", later_file)
        assert result.returncode != 0 and "CHECKPOINT_SCOPE_SCHEDULING_MISSING" in result.stderr, result
        print("PASS: earlier package cannot cover later study")


if __name__ == "__main__":
    main()
