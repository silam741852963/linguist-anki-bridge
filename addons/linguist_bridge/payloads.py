# SPDX-License-Identifier: GPL-3.0-or-later
"""Strict typed `labMutate` bodies for every lab-native-v1 variant.

Each envelope is exactly `{schema_version: 1, variant, body}`. Validation is
structural and bounded: it makes stored intent unambiguous and tamper-evident.
Collection preconditions (model, deck, source content, absence) are checked
again by the native preflight inside the serialized critical section.
"""

from .protocol import _digest

VOCAB_FIELDS = frozenset({
    "Expression", "Reading", "Pronunciation", "Meaning", "Usage", "Examples",
    "Picture", "Audio", "Kanji", "PersonalNotes", "Source", "Language",
    "SenseKey", "EnableProduction", "EnableSpelling", "ProductionPrompt",
    "SpellingPrompt", "ExplanationLanguage",
})
GRAMMAR_FIELDS = frozenset({
    "Pattern", "Meaning", "Formation", "Usage", "Examples", "ExercisePrompt",
    "ExerciseAnswer", "Audio", "PersonalNotes", "Source", "Language",
    "EnableApplication", "UseKey", "RecognitionPrompt", "ExplanationLanguage",
})
FIELDS = {"Linguist Vocabulary v2": VOCAB_FIELDS, "Linguist Grammar v2": GRAMMAR_FIELDS}
BODY_KEYS = frozenset({
    "model_name", "model_manifest_digest", "deck_id", "fields", "tags",
    "marker_tag", "source_plan_digest", "checkpoint_digest", "binding",
    "expected_absent",
})
MAX_ID = 9_007_199_254_740_991
MAX_FIELD_BYTES = 262_144
MAX_MEDIA_BYTES = 256 * 1024 * 1024
PRECONDITION_PREFIX = "lab-jcs-v1:lab-apply-precondition-v1:"


class PayloadError(ValueError):
    pass


def _invalid():
    return PayloadError("BRIDGE_OPERATION_BODY_INVALID")


def _hash(value):
    try:
        return _digest(value)
    except ValueError:
        raise _invalid() from None


APPROVAL_KINDS = ("plan", "checkpoint", "model-install", "lab-restore-decision-v1")
RESTORE = "lab-restore-decision-v1"
# A reviewed plan authorizes forward effects; an observed-state-bound restore
# decision authorizes reverse effects (and re-storing archived media).
VARIANT_APPROVAL = {
    "create_note": ("plan",),
    "update_note": ("plan",),
    "store_media": ("plan", RESTORE),
    "restore_note": (RESTORE,),
    "delete_unstudied_created_note": (RESTORE,),
    "export_checkpoint": ("checkpoint",),
    "install_model": ("model-install",),
}


def _plan_digest(value, kind="plan"):
    prefix = f"lab-jcs-v1:{kind}:"
    if type(value) is not str or not value.startswith(prefix):
        raise _invalid()
    _hash(value.removeprefix(prefix))
    return value


def approval_digest(value):
    """Any approval kind: a reviewed plan, a checkpoint export or a managed
    model install. Each variant accepts only its own kind."""
    for kind in APPROVAL_KINDS:
        if type(value) is str and value.startswith(f"lab-jcs-v1:{kind}:"):
            return _plan_digest(value, kind)
    raise _invalid()


def _precondition(value):
    if type(value) is not str or not value.startswith(PRECONDITION_PREFIX):
        raise _invalid()
    _hash(value.removeprefix(PRECONDITION_PREFIX))
    return value


def _note_id(value):
    if (type(value) is not str or not 1 <= len(value) <= 16 or value[0] == "0"
            or not value.isascii() or not value.isdecimal()
            or not 1 <= int(value) <= MAX_ID):
        raise _invalid()


def _int_id(value):
    if type(value) is not int or not 1 <= value <= MAX_ID:
        raise _invalid()
    return value


def _ordinal(value):
    if type(value) is not int or not 0 <= value < 1000:
        raise _invalid()
    return value


def _exact(body, keys):
    if type(body) is not dict or set(body) != set(keys):
        raise _invalid()


def _text(value, limit):
    if (type(value) is not str or len(value.encode("utf-8")) > limit or "\x00" in value):
        raise _invalid()
    return value


def _label(value, limit=255):
    if (type(value) is not str or not value.strip() or len(value.encode("utf-8")) > limit
            or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in value)):
        raise _invalid()
    return value


def _tags(tags, *, minimum=0):
    if (type(tags) is not list or not minimum <= len(tags) <= 1000
            or any(type(tag) is not str or not 1 <= len(tag.encode("utf-8")) <= 100
                   or any(char.isspace() or ord(char) < 32 or 127 <= ord(char) <= 159
                          for char in tag)
                   for tag in tags)
            or len(set(tags)) != len(tags)):
        raise _invalid()
    return tags


def _field_map(fields):
    if type(fields) is not dict or not 1 <= len(fields) <= 128:
        raise _invalid()
    for name, value in fields.items():
        _label(name)
        _text(value, MAX_FIELD_BYTES)
    return fields


def media_name(name):
    """Collection media name: one path component, no hidden/reserved forms."""
    if (type(name) is not str or not name or len(name.encode("utf-8")) > 255
            or name in (".", "..") or name.startswith(".") or name != name.strip()
            or name.endswith(".") or any(char in name for char in '/\\:*|"<>?')
            or any(ord(char) < 32 or ord(char) == 127 for char in name)):
        raise _invalid()
    return name


def _migration(value):
    if value is None:
        return None
    _exact(value, {"source_model_id", "target_model_id", "target_model_name", "ordinal_map"})
    _int_id(value["source_model_id"])
    _int_id(value["target_model_id"])
    _label(value["target_model_name"])
    entries = value["ordinal_map"]
    if type(entries) is not list or not 1 <= len(entries) <= 64:
        raise _invalid()
    sources, targets = set(), set()
    for entry in entries:
        _exact(entry, {"source", "target"})
        sources.add(_ordinal(entry["source"]))
        targets.add(_ordinal(entry["target"]))
    if len(sources) != len(entries) or len(targets) != len(entries):
        raise _invalid()
    return value


def _create_note(body, operation_id, approved_digest):
    _exact(body, BODY_KEYS)
    model = body["model_name"]
    if type(model) is not str or model not in FIELDS:
        raise _invalid()
    _hash(body["model_manifest_digest"])
    _hash(body["checkpoint_digest"])
    if _plan_digest(body["source_plan_digest"]) != approved_digest:
        raise _invalid()
    binding = body["binding"]
    _exact(binding, {"profile_fingerprint", "path_fingerprint"})
    _hash(binding["profile_fingerprint"])
    _hash(binding["path_fingerprint"])
    _note_id(body["deck_id"])
    marker = "lab_op_" + operation_id.replace("-", "")
    if body["marker_tag"] != marker or body["expected_absent"] is not True:
        raise _invalid()
    fields = body["fields"]
    if type(fields) is not dict or set(fields) != FIELDS[model]:
        raise _invalid()
    for value in fields.values():
        _text(value, MAX_FIELD_BYTES)
    primary = "Expression" if model == "Linguist Vocabulary v2" else "Pattern"
    if (not fields[primary].strip() or not fields["Meaning"].strip()
            or fields["Language"] not in {"ja", "en"}
            or (model == "Linguist Grammar v2" and
                (not fields["UseKey"].strip() or not fields["Formation"].strip()
                 or not fields["Examples"].strip()))):
        raise _invalid()
    switches = (("EnableProduction", "EnableSpelling") if model == "Linguist Vocabulary v2"
                else ("EnableApplication",))
    if any(fields[key] not in ("", "1") for key in switches):
        raise _invalid()
    tags = body["tags"]
    if type(tags) is not list or not 1 <= len(tags) <= 100:
        raise _invalid()
    _tags(tags)
    if marker not in tags:
        raise _invalid()


def _update_note(body, *_):
    _exact(body, {"note_id", "expected_pre_digest", "migration", "fields", "add_tags",
                  "deck_id"})
    _int_id(body["note_id"])
    _precondition(body["expected_pre_digest"])
    _migration(body["migration"])
    _field_map(body["fields"])
    _tags(body["add_tags"])
    _int_id(body["deck_id"])


def _restore_note(body, *_):
    _exact(body, {"note_id", "expected_pre_digest", "migration", "fields", "tags",
                  "card_decks", "removed_card_ids"})
    _int_id(body["note_id"])
    _precondition(body["expected_pre_digest"])
    _migration(body["migration"])
    _field_map(body["fields"])
    _tags(body["tags"])
    decks = body["card_decks"]
    if type(decks) is not list or not 1 <= len(decks) <= 64:
        raise _invalid()
    kept = set()
    for entry in decks:
        _exact(entry, {"card_id", "deck_id"})
        kept.add(_int_id(entry["card_id"]))
        _int_id(entry["deck_id"])
    removed = body["removed_card_ids"]
    if type(removed) is not list or len(removed) > 64:
        raise _invalid()
    removed = {_int_id(card) for card in removed}
    if len(kept) != len(decks) or len(removed) != len(body["removed_card_ids"]) or kept & removed:
        raise _invalid()


def _delete_created(body, *_):
    _exact(body, {"note_id", "expected_pre_digest"})
    _int_id(body["note_id"])
    _precondition(body["expected_pre_digest"])


def _store_media(body, *_):
    _exact(body, {"filename", "sha256", "size_bytes", "staged_asset"})
    media_name(body["filename"])
    _hash(body["sha256"])
    if body["staged_asset"] != body["sha256"]:
        raise _invalid()
    if type(body["size_bytes"]) is not int or not 0 <= body["size_bytes"] <= MAX_MEDIA_BYTES:
        raise _invalid()


def _install_model(body, *_):
    _exact(body, {"manifest", "manifest_digest", "expected_absent"})
    manifest = body["manifest"]
    _exact(manifest, {"name", "version", "fields", "templates", "css"})
    if manifest["name"] not in FIELDS:
        raise _invalid()
    if type(manifest["version"]) is not int or not 1 <= manifest["version"] <= 1000:
        raise _invalid()
    fields = manifest["fields"]
    if type(fields) is not list or set(fields) != FIELDS[manifest["name"]] \
            or len(fields) != len(FIELDS[manifest["name"]]):
        raise _invalid()
    templates = manifest["templates"]
    if type(templates) is not list or not 1 <= len(templates) <= 16:
        raise _invalid()
    ordinals, names = set(), set()
    for template in templates:
        _exact(template, {"name", "ordinal", "front", "back"})
        names.add(_label(template["name"]))
        ordinals.add(_ordinal(template["ordinal"]))
        _text(template["front"], MAX_FIELD_BYTES)
        _text(template["back"], MAX_FIELD_BYTES)
    if ordinals != set(range(len(templates))) or len(names) != len(templates):
        raise _invalid()
    _text(manifest["css"], MAX_FIELD_BYTES)
    _hash(body["manifest_digest"])
    if body["expected_absent"] is not True:
        raise _invalid()


def _export_checkpoint(body, *_):
    _exact(body, {"include_media", "include_scheduling"})
    if type(body["include_media"]) is not bool or body["include_scheduling"] is not True:
        raise _invalid()


VALIDATORS = {
    "create_note": _create_note,
    "update_note": _update_note,
    "restore_note": _restore_note,
    "delete_unstudied_created_note": _delete_created,
    "store_media": _store_media,
    "install_model": _install_model,
    "export_checkpoint": _export_checkpoint,
}


def validate_body(variant, body, *, operation_id, approved_digest):
    """Exact bounded body for one variant; grants no dispatch by itself."""
    validator = VALIDATORS.get(variant)
    if validator is None:
        raise PayloadError("BRIDGE_OPERATION_VARIANT_UNAVAILABLE")
    if not any(type(approved_digest) is str
               and approved_digest.startswith(f"lab-jcs-v1:{kind}:")
               for kind in VARIANT_APPROVAL[variant]):
        raise _invalid()
    approval_digest(approved_digest)
    validator(body, operation_id, approved_digest)
    return None
