# SPDX-License-Identifier: GPL-3.0-or-later
"""Bounded read-only note/card/review/media snapshot. No API registration."""

import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import stat
import threading


class InspectionError(RuntimeError):
    pass


MAX_ID = 9_007_199_254_740_991
MAX_CARDS = 16
MAX_REVIEWS_PER_CARD = 10_000
MAX_RESULT_BYTES = 2 * 1024 * 1024
MAX_MEDIA_BYTES = 1024 * 1024
MAX_MEDIA_REFERENCES = 256


def _wire_id(value):
    if (type(value) is not str or not value.isascii() or not value.isdecimal()
            or value.startswith("0") or not 1 <= len(value) <= 16):
        raise InspectionError("BRIDGE_INSPECT_ID_INVALID")
    number = int(value)
    if not 1 <= number <= MAX_ID:
        raise InspectionError("BRIDGE_INSPECT_ID_INVALID")
    return number


def _media_name(name):
    if (type(name) is not str or not name or len(name.encode("utf-8")) > 255
            or name in (".", "..") or "/" in name or "\\" in name
            or any(ord(char) < 32 or ord(char) == 127 for char in name)):
        raise InspectionError("BRIDGE_INSPECT_MEDIA_NAME_INVALID")
    return name


def _media_observations(collection, note, field_names):
    occurrences = {}
    total = 0
    for field, value in zip(field_names, note.fields):
        for name in collection.media.files_in_str(note.mid, value):
            name = _media_name(name)
            total += 1
            if total > MAX_MEDIA_REFERENCES:
                raise InspectionError("BRIDGE_INSPECT_MEDIA_LIMIT")
            occurrences.setdefault(name, []).append(field)
    if not occurrences:
        return []
    if len({name.casefold() for name in occurrences}) != len(occurrences):
        raise InspectionError("BRIDGE_INSPECT_MEDIA_NAME_CONFLICT")
    directory_path = Path(collection.media.dir())
    if not directory_path.is_absolute() or directory_path.resolve() != directory_path:
        raise InspectionError("BRIDGE_INSPECT_MEDIA_DIR_INVALID")
    directory = os.open(directory_path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        if not stat.S_ISDIR(os.fstat(directory).st_mode):
            raise InspectionError("BRIDGE_INSPECT_MEDIA_DIR_INVALID")
        observed = []
        total_bytes = 0
        for name, fields in occurrences.items():
            try:
                file = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=directory)
            except FileNotFoundError:
                observed.append({"name": name, "fields": fields, "missing": True,
                                 "size_bytes": None, "sha256": None,
                                 "bytes_base64": None})
                continue
            except OSError:
                raise InspectionError("BRIDGE_INSPECT_MEDIA_FILE_INVALID") from None
            try:
                before = os.fstat(file)
                if (not stat.S_ISREG(before.st_mode) or before.st_uid != os.getuid()
                        or before.st_nlink != 1 or before.st_size > MAX_MEDIA_BYTES):
                    raise InspectionError("BRIDGE_INSPECT_MEDIA_FILE_INVALID")
                data = os.read(file, MAX_MEDIA_BYTES + 1)
                after = os.fstat(file)
                path_after = os.stat(name, dir_fd=directory, follow_symlinks=False)
                if (len(data) > MAX_MEDIA_BYTES or len(data) != before.st_size
                        or (before.st_dev, before.st_ino, before.st_mtime_ns, before.st_ctime_ns)
                        != (after.st_dev, after.st_ino, after.st_mtime_ns, after.st_ctime_ns)
                        or (before.st_dev, before.st_ino) != (path_after.st_dev, path_after.st_ino)):
                    raise InspectionError("BRIDGE_INSPECT_MEDIA_CHANGED")
                total_bytes += len(data)
                if total_bytes > MAX_MEDIA_BYTES:
                    raise InspectionError("BRIDGE_INSPECT_MEDIA_LIMIT")
                observed.append({"name": name, "fields": fields, "missing": False,
                                 "size_bytes": len(data),
                                 "sha256": hashlib.sha256(data).hexdigest(),
                                 "bytes_base64": base64.b64encode(data).decode("ascii")})
            finally:
                os.close(file)
        return observed
    finally:
        os.close(directory)


def inspect_note(collection, note_id):
    """Observe one standard note without mutation or collection-wide claims.

    Must be called on Anki's main thread after session validation. The result is
    bounded but is not atomic with other backend workers. Media discovery is
    limited to Anki's field-reference parser and does not cover model/CSS media.
    """
    if threading.current_thread() is not threading.main_thread():
        raise InspectionError("BRIDGE_INSPECT_THREAD_INVALID")
    number = _wire_id(note_id)
    if collection is None:
        raise InspectionError("BRIDGE_INSPECT_COLLECTION_UNAVAILABLE")
    try:
        if collection.db.scalar("select count(*) from notes where id = ?", number) != 1:
            raise InspectionError("BRIDGE_INSPECT_NOTE_NOT_FOUND")
        note = collection.get_note(number)
        model = collection.models.get(note.mid)
        if model is None or model.get("type") != 0:
            raise InspectionError("BRIDGE_INSPECT_MODEL_UNSUPPORTED")
        if note.id != number or model.get("id") != note.mid:
            raise InspectionError("BRIDGE_INSPECT_MODEL_INVALID")
        fields = model.get("flds")
        templates = model.get("tmpls")
        if (type(fields) is not list or not 1 <= len(fields) <= 128
                or type(templates) is not list or not 1 <= len(templates) <= MAX_CARDS
                or len(note.fields) != len(fields)):
            raise InspectionError("BRIDGE_INSPECT_MODEL_INVALID")
        field_names = [field["name"] for field in fields]
        if len(set(field_names)) != len(field_names):
            raise InspectionError("BRIDGE_INSPECT_MODEL_INVALID")
        if any(len(value.encode("utf-8")) > MAX_RESULT_BYTES for value in note.fields):
            raise InspectionError("BRIDGE_INSPECT_RESULT_LIMIT")
        template_rows = [
            {"ordinal": index, "name": template["name"],
             "front": template["qfmt"], "back": template["afmt"]}
            for index, template in enumerate(templates)
        ]
        ids = list(collection.card_ids_of_note(number))
        if len(ids) > MAX_CARDS or len(ids) != len(set(ids)):
            raise InspectionError("BRIDGE_INSPECT_CARD_LIMIT")
        cards = []
        total_reviews = 0
        for card_id in ids:
            if not 1 <= int(card_id) <= MAX_ID:
                raise InspectionError("BRIDGE_INSPECT_CARD_INVALID")
            card = collection.get_card(card_id)
            if (card.nid != number or card.id != card_id
                    or not 0 <= card.ord < len(templates)
                    or not 1 <= int(card.did) <= MAX_ID
                    or not 0 <= int(card.odid) <= MAX_ID):
                raise InspectionError("BRIDGE_INSPECT_CARD_INVALID")
            reviews = collection.db.all(
                "select id,cid,usn,ease,ivl,lastIvl,factor,time,type "
                "from revlog where cid = ? order by id limit ?",
                card_id, MAX_REVIEWS_PER_CARD + 1)
            if len(reviews) > MAX_REVIEWS_PER_CARD:
                raise InspectionError("BRIDGE_INSPECT_REVIEW_LIMIT")
            total_reviews += len(reviews)
            if total_reviews > MAX_REVIEWS_PER_CARD:
                raise InspectionError("BRIDGE_INSPECT_REVIEW_LIMIT")
            cards.append({
                "id": str(card.id), "ordinal": card.ord,
                "deck_id": str(card.did), "original_deck_id": str(card.odid),
                "type": int(card.type), "queue": int(card.queue),
                "due": card.due, "interval": card.ivl,
                "ease_factor": card.factor, "repetitions": card.reps,
                "lapses": card.lapses, "remaining_steps": card.left,
                "original_due": card.odue, "flags": card.flags,
                "custom_data": card.custom_data,
                "memory_state_hex": (
                    card.memory_state.SerializeToString().hex()
                    if card.memory_state is not None else None),
                "desired_retention": card.desired_retention,
                "decay": card.decay,
                "last_review_time": card.last_review_time,
                "reviews": [dict(zip(
                    ("id", "card_id", "usn", "ease", "interval",
                     "previous_interval", "factor", "time", "type"), row))
                    for row in reviews],
            })
        cards.sort(key=lambda card: (card["ordinal"], int(card["id"])))
        media = _media_observations(collection, note, field_names)
        result = {
            "schema_version": 1,
            "note_id": str(note.id),
            "note": {"guid": note.guid, "model_id": str(note.mid),
                     "fields": dict(zip(field_names, note.fields)),
                     "tags": list(note.tags)},
            "model": {"id": str(model["id"]), "name": model["name"],
                      "fields": field_names, "templates": template_rows,
                      "css": model["css"]},
            "model_payload": copy.deepcopy(model),
            "cards": cards,
            "media": media,
            "review_rows_untruncated": True,
            "discovered_media_bytes_verified": all(not item["missing"] for item in media),
            "media_references_complete": False,
            "model_manifest_complete": False,
            "atomic_snapshot_verified": False,
            "write_authorized": False,
        }
        encoded = json.dumps(result, ensure_ascii=False, allow_nan=False,
                             separators=(",", ":")).encode("utf-8")
        if len(encoded) > MAX_RESULT_BYTES:
            raise InspectionError("BRIDGE_INSPECT_RESULT_LIMIT")
        return result
    except InspectionError:
        raise
    except Exception:
        raise InspectionError("BRIDGE_INSPECT_UNAVAILABLE") from None


def inspect_note_twice(collection, note_id):
    """Reject observed drift; matching reads still are not a native transaction."""
    first = inspect_note(collection, note_id)
    second = inspect_note(collection, note_id)
    if first != second:
        raise InspectionError("BRIDGE_INSPECT_SOURCE_DRIFT")
    second["repeated_reads_matched"] = True
    return second
