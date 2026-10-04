# SPDX-License-Identifier: GPL-3.0-or-later
"""Native collection effects for typed `labMutate` bodies (WP-11).

These functions operate on an already-open Anki `Collection` passed by the
caller. They import no Anki/Qt module, register no action and are not part of
the read-only add-on artifact. No dispatcher calls them yet: the serialized
main-thread critical section, ledger linkage and owner/fence checks remain
pending, so collection writes stay disabled. They exist so that disposable
Anki tests can prove the exact native effect semantics the Rust apply
orchestration expects (retained card IDs, history, scheduling, no overwrite).
"""
import hashlib
import json
import os
import stat

FORMAT = "lab-jcs-v1"
SCHEDULER_KEYS = ("queue", "type", "due", "ivl", "factor", "reps", "lapses", "left",
                  "odue", "flags")


class EffectError(RuntimeError):
    pass


def jcs(value):
    """RFC 8785 bytes for the ASCII-keyed, integer/string values used here."""
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def domain_digest(domain, value):
    """Same construction as `linguist_core::canonical::digest`."""
    digest = hashlib.sha256(FORMAT.encode() + b"\0" + domain.encode() + b"\0" + jcs(value))
    return f"{FORMAT}:{domain}:{digest.hexdigest()}"


def _history_digest(col, card_id):
    rows = col.db.all(
        "select id, cid, usn, ease, ivl, lastIvl, factor, time, type "
        "from revlog where cid = ? order by id", card_id)
    return hashlib.sha256(jcs(rows)).hexdigest(), len(rows)


def observe_note(col, note_id, manifest_digest):
    """ObservedNote shape consumed by the Rust apply port."""
    note = col.get_note(note_id)
    model = col.models.get(note.mid)
    cards = []
    for card_id in col.card_ids_of_note(note_id):
        card = col.get_card(card_id)
        history, count = _history_digest(col, card_id)
        scheduler = {key: str(getattr(card, key)) for key in SCHEDULER_KEYS}
        scheduler["memory_state"] = str(card.memory_state)
        cards.append({
            "id": card.id, "ordinal": card.ord, "deck_id": card.did,
            "original_deck_id": card.odid, "scheduler": scheduler,
            "history_digest": history, "review_count": count,
        })
    cards.sort(key=lambda card: card["id"])
    return {
        "id": note.id, "model_id": note.mid, "model_name": model["name"],
        "model_manifest_digest": manifest_digest(model),
        "fields": {name: note[name] for name in note.keys()},
        "tags": list(note.tags), "cards": cards,
    }


def content_digest(observed):
    """Same projection as `linguist_application::apply::content_digest`."""
    cards = sorted([card["id"], card["ordinal"], card["deck_id"], card["original_deck_id"]]
                   for card in observed["cards"])
    return domain_digest("lab-apply-precondition-v1", [
        observed["model_name"], observed["model_manifest_digest"], observed["fields"],
        sorted(set(observed["tags"])), cards,
    ])


def create_note(col, body, manifest_digest):
    """create_note: exact model manifest, target deck, marker must be absent."""
    marker = body["marker_tag"]
    if body.get("expected_absent") is not True or col.find_notes(f"tag:{marker}"):
        raise EffectError("BRIDGE_PRECONDITION_FAILED")
    model = col.models.by_name(body["model_name"])
    if model is None or manifest_digest(model) != body["model_manifest_digest"]:
        raise EffectError("BRIDGE_MODEL_MISMATCH")
    deck_id = int(body["deck_id"])
    deck = col.decks.get(deck_id, default=False)
    if not deck or deck.get("dyn"):
        raise EffectError("BRIDGE_DECK_INVALID")
    note = col.new_note(model)
    if set(note.keys()) != set(body["fields"]):
        raise EffectError("BRIDGE_FIELDS_MISMATCH")
    for name, value in body["fields"].items():
        note[name] = value
    note.tags = list(body["tags"])
    col.add_note(note, deck_id)
    return note.id


def update_note(col, body, manifest_digest):
    """update_note: precondition CAS, optional mapped migration, fields, added
    tags and deck placement. Retained cards keep their IDs and history."""
    observed = observe_note(col, body["note_id"], manifest_digest)
    if content_digest(observed) != body["expected_pre_digest"]:
        raise EffectError("BRIDGE_PRECONDITION_FAILED")
    if any(card["original_deck_id"] for card in observed["cards"]):
        raise EffectError("BRIDGE_FILTERED_DECK")
    migration = body.get("migration")
    if migration is not None:
        source = col.models.get(migration["source_model_id"])
        target = col.models.get(migration["target_model_id"])
        if (source is None or target is None or observed["model_id"] != source["id"]
                or target["name"] != migration["target_model_name"]):
            raise EffectError("BRIDGE_MODEL_MISMATCH")
        mapping = {entry["target"]: entry["source"] for entry in migration["ordinal_map"]}
        if {card["ordinal"] for card in observed["cards"]} - set(mapping.values()):
            raise EffectError("BRIDGE_MIGRATION_DROPS_CARD")
        info = col.models.change_notetype_info(
            old_notetype_id=source["id"], new_notetype_id=target["id"])
        request = info.input
        request.note_ids.append(body["note_id"])
        del request.new_fields[:]
        request.new_fields.extend(-1 for _ in target["flds"])
        del request.new_templates[:]
        request.new_templates.extend(mapping.get(index, -1)
                                     for index in range(len(target["tmpls"])))
        col.models.change_notetype_of_notes(request)
    note = col.get_note(body["note_id"])
    if set(note.keys()) != set(body["fields"]):
        raise EffectError("BRIDGE_FIELDS_MISMATCH")
    for name, value in body["fields"].items():
        note[name] = value
    for tag in body["add_tags"]:
        if tag not in note.tags:
            note.tags.append(tag)
    col.update_note(note)
    card_ids = col.card_ids_of_note(body["note_id"])
    col.set_deck(card_ids, int(body["deck_id"]))
    return body["note_id"]


def _safe_name(name):
    if (type(name) is not str or not name or len(name.encode("utf-8")) > 255
            or name.startswith(".") or "/" in name or "\\" in name or ":" in name
            or any(ord(char) < 32 or ord(char) == 127 for char in name)):
        raise EffectError("BRIDGE_MEDIA_NAME_INVALID")
    return name


def media_sha256(col, name):
    path = os.path.join(col.media.dir(), _safe_name(name))
    try:
        info = os.lstat(path)
    except FileNotFoundError:
        return None
    if not stat.S_ISREG(info.st_mode):
        raise EffectError("BRIDGE_MEDIA_FILE_INVALID")
    with open(path, "rb") as file:
        return hashlib.sha256(file.read()).hexdigest()


def store_media(col, name, data, sha256):
    """store_media: verified bytes under the exact name; never overwrites."""
    if hashlib.sha256(data).hexdigest() != sha256:
        raise EffectError("BRIDGE_MEDIA_HASH_MISMATCH")
    existing = media_sha256(col, name)
    if existing == sha256:
        return name
    if existing is not None:
        raise EffectError("BRIDGE_MEDIA_COLLISION")
    stored = col.media.write_data(name, data)
    if stored != name or media_sha256(col, name) != sha256:
        raise EffectError("BRIDGE_MEDIA_STORE_UNVERIFIED")
    return name
