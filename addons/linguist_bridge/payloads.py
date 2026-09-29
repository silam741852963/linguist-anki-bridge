# SPDX-License-Identifier: GPL-3.0-or-later
"""Strict stored intent shape for native operations; no Anki calls or dispatch."""

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


class PayloadError(ValueError):
    pass


def _hash(value):
    try:
        return _digest(value)
    except ValueError:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID") from None


def _plan_digest(value):
    if type(value) is not str or not value.startswith("lab-jcs-v1:plan:"):
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    _hash(value.removeprefix("lab-jcs-v1:plan:"))
    return value


def _note_id(value):
    if (type(value) is not str or not 1 <= len(value) <= 16 or value[0] == "0"
            or not value.isascii() or not value.isdecimal()
            or not 1 <= int(value) <= 9007199254740991):
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")


def validate_body(variant, body, *, operation_id, approved_digest):
    """Accept only structurally complete, bounded create-note intent for now.

    Collection/content preconditions, Anki model/deck state, checkpoint bytes and
    media references require a later native preflight; this grants no dispatch.
    """
    if variant != "create_note":
        raise PayloadError("BRIDGE_OPERATION_VARIANT_UNAVAILABLE")
    if type(body) is not dict or set(body) != BODY_KEYS:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    model = body["model_name"]
    if type(model) is not str or model not in FIELDS:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    _hash(body["model_manifest_digest"])
    _hash(body["checkpoint_digest"])
    if _plan_digest(body["source_plan_digest"]) != approved_digest:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    binding = body["binding"]
    if type(binding) is not dict or set(binding) != {"profile_fingerprint", "path_fingerprint"}:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    _hash(binding["profile_fingerprint"])
    _hash(binding["path_fingerprint"])
    _note_id(body["deck_id"])
    marker = "lab_op_" + operation_id.replace("-", "")
    if body["marker_tag"] != marker or body["expected_absent"] is not True:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    fields = body["fields"]
    if type(fields) is not dict or set(fields) != FIELDS[model]:
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    if any(type(value) is not str or len(value.encode("utf-8")) > 262144
           or "\x00" in value for value in fields.values()):
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    primary = "Expression" if model == "Linguist Vocabulary v2" else "Pattern"
    if (not fields[primary].strip() or not fields["Meaning"].strip()
            or fields["Language"] not in {"ja", "en"}
            or (model == "Linguist Grammar v2" and
                (not fields["UseKey"].strip() or not fields["Formation"].strip()
                 or not fields["Examples"].strip()))):
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    switches = ("EnableProduction", "EnableSpelling") if model == "Linguist Vocabulary v2" else ("EnableApplication",)
    if any(fields[key] not in ("", "1") for key in switches):
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    tags = body["tags"]
    if (type(tags) is not list or not 1 <= len(tags) <= 100
            or any(type(tag) is not str or not 1 <= len(tag.encode("utf-8")) <= 100
                   or any(char.isspace() or ord(char) < 32 or 127 <= ord(char) <= 159 for char in tag)
                   for tag in tags)
            or len(set(tags)) != len(tags) or marker not in tags):
        raise PayloadError("BRIDGE_OPERATION_BODY_INVALID")
    return None
