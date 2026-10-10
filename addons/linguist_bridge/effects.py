# SPDX-License-Identifier: GPL-3.0-or-later
"""Native collection effects for typed `labMutate` bodies (WP-03, WP-11, WP-12).

Every variant is split into a read-only `preflight_*` that raises
`EffectError` before any write, and a `perform_*` that writes. The runtime
records `running` durably between the two, inside one serialized main-thread
critical section, so a refusal is provably before-write and a failure after
`running` is reported as unknown, never as success. Functions take an
already-open Anki `Collection`; they import no Anki/Qt module.
"""
import hashlib
import json
import os
import stat

from . import manifest as canonical_manifest

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


def _memory_state(card):
    state = getattr(card, "memory_state", None)
    if state is None:
        return "None"
    return f"stability={state.stability!r};difficulty={state.difficulty!r}"


def observe_note(col, note_id, manifest_digest=None):
    """ObservedNote shape consumed by the Rust apply port."""
    manifest_digest = manifest_digest or canonical_manifest.model_digest
    note = col.get_note(note_id)
    model = col.models.get(note.mid)
    cards = []
    for card_id in col.card_ids_of_note(note_id):
        card = col.get_card(card_id)
        history, count = _history_digest(col, card_id)
        scheduler = _scheduler(card)
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


def observe_or_none(col, note_id, manifest_digest=None):
    try:
        exists = col.db.scalar("select count() from notes where id = ?", int(note_id))
    except Exception:  # Fake collections in unit tests have no notes table.
        exists = None
    if exists == 0:
        return None
    try:
        return observe_note(col, int(note_id), manifest_digest)
    except Exception:  # Anki raises NotFoundError for a missing note.
        if exists:
            raise
        return None


def content_digest(observed):
    """Same projection as `linguist_application::apply::content_digest`."""
    cards = sorted([card["id"], card["ordinal"], card["deck_id"], card["original_deck_id"]]
                   for card in observed["cards"])
    return domain_digest("lab-apply-precondition-v1", [
        observed["model_name"], observed["model_manifest_digest"], observed["fields"],
        sorted(set(observed["tags"])), cards,
    ])


def _deck(col, deck_id):
    deck = col.decks.get(int(deck_id), default=False)
    if not deck or deck.get("dyn"):
        raise EffectError("BRIDGE_DECK_INVALID")
    return deck


def _studied(card):
    return card["review_count"] > 0 or card["scheduler"].get("reps", "0") != "0"


# ------------------------------------------------------------------ create_note
def tag_search(tag):
    """Exact-tag search term; `_` is a single-character wildcard in Anki."""
    return '"tag:' + tag.replace("\\", "\\\\").replace("_", "\\_") + '"'


def notes_tagged(col, tag):
    found = []
    for note_id in sorted(col.find_notes(tag_search(tag))):
        observed = observe_note(col, note_id)
        if tag in observed["tags"]:
            found.append(observed)
    return found


def _scheduler(card):
    scheduler = {key: str(getattr(card, key)) for key in SCHEDULER_KEYS}
    scheduler["memory_state"] = _memory_state(card)
    return scheduler


def _inherit_sources(col, body, model):
    """WP-23: each source card exists outside filtered decks, still has the
    scheduler the intent was built from, and names a template of the model."""
    sources = []
    for entry in body.get("inherit_schedule", ()):
        if entry["card_ordinal"] >= len(model["tmpls"]):
            raise EffectError("BRIDGE_INHERIT_ORDINAL_INVALID")
        try:
            source = col.get_card(entry["source_card_id"])
        except Exception as error:  # Anki raises NotFoundError for a missing card.
            raise EffectError("BRIDGE_PRECONDITION_FAILED") from error
        if source.odid:
            raise EffectError("BRIDGE_FILTERED_DECK")
        if _scheduler(source) != entry["scheduler"]:
            raise EffectError("BRIDGE_PRECONDITION_FAILED")
        sources.append((entry["card_ordinal"], source))
    return sources


def _inherit(col, note_id, sources):
    """Copy each source card's schedule into the new card at the same task:
    queue, type, due, interval, ease, learning steps and FSRS state. The new
    card keeps no reviews, lapses or flag of its own, so it stays unstudied."""
    cards = {col.get_card(card_id).ord: card_id for card_id in col.card_ids_of_note(note_id)}
    for ordinal, source in sources:
        if ordinal not in cards:
            raise EffectError("BRIDGE_INHERIT_CARD_MISSING")
        card = col.get_card(cards[ordinal])
        for key in ("queue", "type", "due", "ivl", "factor", "left"):
            setattr(card, key, getattr(source, key))
        card.memory_state = getattr(source, "memory_state", None)
        for key in ("desired_retention", "decay"):
            if hasattr(source, key):
                setattr(card, key, getattr(source, key))
        col.update_card(card)


def preflight_create_note(col, body, manifest_digest=None):
    manifest_digest = manifest_digest or canonical_manifest.model_digest
    marker = body["marker_tag"]
    if body.get("expected_absent") is not True or col.find_notes(tag_search(marker)):
        raise EffectError("BRIDGE_PRECONDITION_FAILED")
    model = col.models.by_name(body["model_name"])
    if model is None or manifest_digest(model) != body["model_manifest_digest"]:
        raise EffectError("BRIDGE_MODEL_MISMATCH")
    _deck(col, body["deck_id"])
    if {field["name"] for field in model["flds"]} != set(body["fields"]):
        raise EffectError("BRIDGE_FIELDS_MISMATCH")
    return model, _inherit_sources(col, body, model)


def perform_create_note(col, body, prepared):
    model, sources = prepared
    note = col.new_note(model)
    for name, value in body["fields"].items():
        note[name] = value
    note.tags = list(body["tags"])
    col.add_note(note, int(body["deck_id"]))
    _inherit(col, note.id, sources)
    return note.id


def create_note(col, body, manifest_digest=None):
    """create_note: exact model manifest, target deck, marker must be absent."""
    return perform_create_note(col, body, preflight_create_note(col, body, manifest_digest))


# ------------------------------------------------------------------ deck moves
def _check_moves(col, card_ids, deck_of):
    """Refuse a deck move that would carry an FSRS memory state into a deck
    with a different preset; the state would no longer match its parameters."""
    for card_id in card_ids:
        card = col.get_card(card_id)
        target = deck_of(card_id)
        if target is None or card.did == target or card.memory_state is None:
            continue
        if (col.decks.config_dict_for_deck_id(card.did)["id"]
                != col.decks.config_dict_for_deck_id(target)["id"]):
            raise EffectError("BRIDGE_FSRS_PRESET_CHANGE")


def _move_cards(col, card_ids, deck_of):
    """Move cards one by one through `update_card`. Anki 25.09's
    `Collection.set_deck` clears FSRS memory state (stability, difficulty,
    desired retention, decay) even between decks sharing a preset; writing
    the card keeps every scheduling field and the review log unchanged."""
    for card_id in card_ids:
        target = deck_of(card_id)
        card = col.get_card(card_id)
        if target is None or card.did == target:
            continue
        card.did = target
        col.update_card(card)


# ------------------------------------------------------------------ migration
def _migration_models(col, migration, observed):
    source = col.models.get(migration["source_model_id"])
    target = col.models.get(migration["target_model_id"])
    if (source is None or target is None or observed["model_id"] != source["id"]
            or target["name"] != migration["target_model_name"]):
        raise EffectError("BRIDGE_MODEL_MISMATCH")
    if any(entry["source"] >= len(source["tmpls"]) or entry["target"] >= len(target["tmpls"])
           for entry in migration["ordinal_map"]):
        raise EffectError("BRIDGE_MIGRATION_MAP_INVALID")
    return source, target


def _change_notetype(col, note_id, migration, source, target):
    """Mapped note-type change; unmapped template ordinals lose their cards."""
    mapping = {entry["target"]: entry["source"] for entry in migration["ordinal_map"]}
    info = col.models.change_notetype_info(
        old_notetype_id=source["id"], new_notetype_id=target["id"])
    request = info.input
    request.note_ids.append(note_id)
    del request.new_fields[:]
    request.new_fields.extend(-1 for _ in target["flds"])
    del request.new_templates[:]
    request.new_templates.extend(mapping.get(index, -1)
                                 for index in range(len(target["tmpls"])))
    col.models.change_notetype_of_notes(request)
    return mapping


def _final_fields(col, observed, migration, target):
    if migration is not None:
        return {field["name"] for field in target["flds"]}
    return set(observed["fields"])


# ------------------------------------------------------------------ update_note
def preflight_update_note(col, body, manifest_digest=None):
    observed = observe_or_none(col, body["note_id"], manifest_digest)
    if observed is None:
        raise EffectError("BRIDGE_NOTE_MISSING")
    if content_digest(observed) != body["expected_pre_digest"]:
        raise EffectError("BRIDGE_PRECONDITION_FAILED")
    if any(card["original_deck_id"] for card in observed["cards"]):
        raise EffectError("BRIDGE_FILTERED_DECK")
    deck_id = int(body["deck_id"])
    _deck(col, deck_id)
    _check_moves(col, [card["id"] for card in observed["cards"]], lambda _: deck_id)
    migration = body.get("migration")
    models = None
    if migration is not None:
        mapped = {entry["source"] for entry in migration["ordinal_map"]}
        if {card["ordinal"] for card in observed["cards"]} - mapped:
            raise EffectError("BRIDGE_MIGRATION_DROPS_CARD")
        models = _migration_models(col, migration, observed)
    if _final_fields(col, observed, migration, models and models[1]) != set(body["fields"]):
        raise EffectError("BRIDGE_FIELDS_MISMATCH")
    return models


def perform_update_note(col, body, models):
    note_id = body["note_id"]
    deck_id = int(body["deck_id"])
    if models is not None:
        _change_notetype(col, note_id, body["migration"], *models)
    note = col.get_note(note_id)
    for name, value in body["fields"].items():
        note[name] = value
    for tag in body["add_tags"]:
        if tag not in note.tags:
            note.tags.append(tag)
    col.update_note(note)
    _move_cards(col, col.card_ids_of_note(note_id), lambda _: deck_id)
    return note_id


def update_note(col, body, manifest_digest=None):
    """update_note: precondition CAS, optional mapped migration, fields, added
    tags and deck placement. Retained cards keep their IDs and history."""
    return perform_update_note(col, body, preflight_update_note(col, body, manifest_digest))


# ------------------------------------------------------------------ restore_note
def preflight_restore_note(col, body, manifest_digest=None):
    try:
        observed = observe_note(col, body["note_id"], manifest_digest)
    except Exception as error:
        raise EffectError("BRIDGE_NOTE_MISSING") from error
    if content_digest(observed) != body["expected_pre_digest"]:
        raise EffectError("BRIDGE_PRECONDITION_FAILED")
    if any(card["original_deck_id"] for card in observed["cards"]):
        raise EffectError("BRIDGE_FILTERED_DECK")
    removed = set(body["removed_card_ids"])
    kept = {entry["card_id"]: int(entry["deck_id"]) for entry in body["card_decks"]}
    migration = body.get("migration")
    models = None
    if migration is None:
        if removed:
            raise EffectError("BRIDGE_REMOVAL_REQUIRES_MAPPING")
    else:
        mapped = {entry["source"] for entry in migration["ordinal_map"]}
        dropped = {card["id"] for card in observed["cards"] if card["ordinal"] not in mapped}
        if dropped != removed:
            raise EffectError("BRIDGE_REVERSE_MAPPING_MISMATCH")
        if any(_studied(card) for card in observed["cards"] if card["id"] in removed):
            raise EffectError("BRIDGE_STUDIED_CARD_REMOVAL")
    if {card["id"] for card in observed["cards"]} - removed != set(kept):
        raise EffectError("BRIDGE_CARD_SET_MISMATCH")
    for deck_id in set(kept.values()):
        _deck(col, deck_id)
    _check_moves(col, list(kept), kept.get)
    if migration is not None:
        models = _migration_models(col, migration, observed)
    if _final_fields(col, observed, migration, models and models[1]) != set(body["fields"]):
        raise EffectError("BRIDGE_FIELDS_MISMATCH")
    return models, kept


def perform_restore_note(col, body, prepared):
    models, kept = prepared
    if models is not None:
        _change_notetype(col, body["note_id"], body["migration"], *models)
    note = col.get_note(body["note_id"])
    for name, value in body["fields"].items():
        note[name] = value
    note.tags = list(body["tags"])
    col.update_note(note)
    _move_cards(col, list(kept), kept.get)
    return body["note_id"]


def restore_note(col, body, manifest_digest=None):
    """restore_note (WP-12): precondition CAS, an optional reverse mapped
    note-type change that removes only the listed unstudied cards, the exact
    field set and tag set, and one deck per kept card. Kept cards keep their
    IDs, current scheduling and review history; nothing is rescheduled."""
    return perform_restore_note(col, body, preflight_restore_note(col, body, manifest_digest))


# ------------------------------------------------------------------ delete
def preflight_delete_unstudied_created_note(col, body, manifest_digest=None):
    try:
        observed = observe_note(col, body["note_id"], manifest_digest)
    except Exception as error:  # Anki raises NotFoundError for a missing note.
        raise EffectError("BRIDGE_NOTE_MISSING") from error
    if content_digest(observed) != body["expected_pre_digest"]:
        raise EffectError("BRIDGE_PRECONDITION_FAILED")
    if any(_studied(card) for card in observed["cards"]):
        raise EffectError("BRIDGE_STUDIED_NOTE")
    return None


def perform_delete_unstudied_created_note(col, body, _prepared):
    col.remove_notes([body["note_id"]])
    return body["note_id"]


def delete_unstudied_created_note(col, body, manifest_digest=None):
    """delete_unstudied_created_note (WP-12): remove one app-created note only
    while its content matches the precondition and no card has any review."""
    preflight_delete_unstudied_created_note(col, body, manifest_digest)
    return perform_delete_unstudied_created_note(col, body, None)


# ------------------------------------------------------------------ media
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


def media_observation(col, name):
    digest = media_sha256(col, name)
    if digest is None:
        return None
    size = os.lstat(os.path.join(col.media.dir(), name)).st_size
    return {"filename": name, "sha256": digest, "size_bytes": size}


def preflight_store_media(col, name, data, sha256):
    if hashlib.sha256(data).hexdigest() != sha256:
        raise EffectError("BRIDGE_MEDIA_HASH_MISMATCH")
    existing = media_sha256(col, name)
    if existing is not None and existing != sha256:
        raise EffectError("BRIDGE_MEDIA_COLLISION")
    return existing == sha256


def perform_store_media(col, name, data, sha256, already_present):
    if already_present:
        return name
    stored = col.media.write_data(name, data)
    if stored != name or media_sha256(col, name) != sha256:
        raise EffectError("BRIDGE_MEDIA_STORE_UNVERIFIED")
    return name


def store_media(col, name, data, sha256):
    """store_media: verified bytes under the exact name; never overwrites."""
    present = preflight_store_media(col, name, data, sha256)
    return perform_store_media(col, name, data, sha256, present)


# ------------------------------------------------------------------ install_model
def preflight_install_model(col, body):
    manifest = body["manifest"]
    if canonical_manifest.managed_digest(manifest) != body["manifest_digest"]:
        raise EffectError("BRIDGE_MANIFEST_DIGEST_MISMATCH")
    if body.get("expected_absent") is not True or col.models.by_name(manifest["name"]) is not None:
        raise EffectError("BRIDGE_MODEL_EXISTS")
    return None


def perform_install_model(col, body, _prepared):
    manifest = body["manifest"]
    model = col.models.new(manifest["name"])
    model["css"] = manifest["css"]
    for name in manifest["fields"]:
        col.models.add_field(model, col.models.new_field(name))
    for template in sorted(manifest["templates"], key=lambda t: t["ordinal"]):
        item = col.models.new_template(template["name"])
        item["qfmt"] = template["front"]
        item["afmt"] = template["back"]
        col.models.add_template(model, item)
    col.models.add_dict(model)
    created = col.models.by_name(manifest["name"])
    if created is None or canonical_manifest.model_digest(created) != body["manifest_digest"]:
        raise EffectError("BRIDGE_MODEL_INSTALL_UNVERIFIED")
    return created["id"]


# ------------------------------------------------------------------ reads
def models_named(col, name):
    out = []
    for entry in col.models.all_names_and_ids():
        if entry.name != name:
            continue
        model = col.models.get(entry.id)
        projection = canonical_manifest.projection_of_model(model)
        out.append({"id": model["id"], "name": model["name"],
                    "fields": projection["fields"], "templates": projection["templates"],
                    "css": model["css"]})
    return out


def deck_named(col, name):
    for entry in col.decks.all_names_and_ids():
        if entry.name == name:
            deck = col.decks.get(entry.id)
            return {"id": deck["id"], "name": deck["name"], "filtered": bool(deck.get("dyn"))}
    return None


def scope_manifest(col, note_ids, requirement, model_ids=()):
    """Checkpoint scope: the notes' cards with their observed scheduling and
    review counts, their models plus the named existing models, and every
    collection media file."""
    notes = sorted({int(n) for n in note_ids})
    cards, models = [], set()
    for model_id in model_ids:
        if col.models.get(int(model_id)) is None:
            raise EffectError("BRIDGE_MODEL_MISSING")
        models.add(int(model_id))
    for note_id in notes:
        note = col.get_note(note_id)
        models.add(note.mid)
        for card_id in col.card_ids_of_note(note_id):
            card = col.get_card(card_id)
            reviews = col.db.scalar("select count() from revlog where cid = ?", card_id)
            cards.append({"card_id": card_id, "note_id": note_id, "reps": card.reps,
                          "review_count": reviews})
    cards.sort(key=lambda card: card["card_id"])
    media = []
    directory = col.media.dir()
    for name in sorted(os.listdir(directory)):
        path = os.path.join(directory, name)
        info = os.lstat(path)
        if not stat.S_ISREG(info.st_mode) or name.startswith("."):
            continue
        with open(path, "rb") as handle:
            media.append({"name": name, "sha1": hashlib.sha1(handle.read()).hexdigest()})
    media.sort(key=lambda item: item["name"].encode("utf-8"))
    return {"schema_version": 1, "requirement": requirement, "note_ids": notes,
            "cards": cards, "model_ids": sorted(models), "media": media}
