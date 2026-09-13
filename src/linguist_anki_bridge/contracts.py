"""Versioned interchange contracts used during the native migration.

The native code and the Python application deliberately exchange plain JSON
instead of importing one another. Keeping this adapter small lets parity tests
compare both implementations while Python remains the production fallback.
"""

from __future__ import annotations

from typing import Any

from .card_model import CardDocument, MediaAsset


CARD_DOCUMENT_VERSION = 1
BATCH_JOB_VERSION = 1
SNAPSHOT_VERSION = 1
LOGICAL_FIELDS = (
    "meaning_image",
    "meaning_text",
    "kanji_construction",
    "audio",
)
PROVENANCE_SOURCES = {"original", "dictionary", "ocr", "generated", "user"}

_BATCH_JOB_FIELDS = (
    "id", "deck_key", "deck_name", "status", "dry_run", "settings",
    "created_at", "updated_at", "started_at", "finished_at", "last_error",
)
_BATCH_ITEM_FIELDS = (
    "id", "ordinal", "note_id", "word", "status", "attempts",
    "next_attempt_at", "snapshot_id", "result_note_id", "last_error",
    "started_at", "finished_at", "updated_at",
)
_BATCH_ITEM_AUXILIARY_FIELDS = ("artifact_path", "artifact_extensions")
_SNAPSHOT_FIELDS = (
    "id", "created_at", "word", "mode", "deck_key", "dry_run", "status",
    "processed", "media_before", "result_note_id", "created_note_ids", "error",
    "reverted_at",
)
_SNAPSHOT_NOTE_FIELDS = ("note_id", "model_name", "deck_name", "tags", "fields")


def _extensions(source: dict[str, Any], known: tuple[str, ...]) -> dict[str, Any]:
    """Move unmodelled historical fields into an explicit lossless namespace."""
    supplied = source.get("extensions", {})
    if not isinstance(supplied, dict):
        raise ValueError("Contract extensions must be an object")
    extras = dict(supplied)
    for name, value in source.items():
        if name not in known and name != "extensions":
            extras[name] = value
    return extras


def _restore_extensions(payload: dict[str, Any], known: tuple[str, ...]) -> dict[str, Any]:
    extensions = payload.get("extensions", {})
    if not isinstance(extensions, dict):
        raise ValueError("Contract extensions must be an object")
    collisions = set(extensions).intersection(known)
    if collisions:
        raise ValueError(f"Contract extensions cannot replace known fields: {sorted(collisions)!r}")
    restored = {name: payload.get(name) for name in known if name in payload}
    restored.update(extensions)
    return restored


def export_card_document(
    document: CardDocument,
    provenance: dict[str, list[dict[str, Any]]] | None = None,
) -> dict[str, Any]:
    """Convert the current Python domain object into contract v1."""
    payload = {
        "schema_version": CARD_DOCUMENT_VERSION,
        "expression": document.expression,
        "values": {name: document.values.get(name) for name in LOGICAL_FIELDS},
        "media": [
            {"filename": asset.filename, "data_base64": asset.data_base64}
            for asset in document.media
        ],
        "obsolete_media": list(document.obsolete_media),
        "issues": list(document.issues),
        "tags": list(document.tags),
        "provenance": provenance or {},
    }
    validate_card_document(payload)
    return payload


def import_card_document(payload: dict[str, Any]) -> CardDocument:
    """Build the existing Python domain object from contract v1."""
    validate_card_document(payload)
    values = payload["values"]
    return CardDocument(
        expression=payload["expression"],
        values={name: values[name] for name in LOGICAL_FIELDS},
        media=[MediaAsset(item["filename"], item["data_base64"]) for item in payload["media"]],
        obsolete_media=list(payload["obsolete_media"]),
        issues=list(payload["issues"]),
        tags=list(payload["tags"]),
    )


def validate_card_document(payload: dict[str, Any]) -> None:
    """Reject incompatible data before either runtime acts on it."""
    if not isinstance(payload, dict):
        raise ValueError("Card document must be an object")
    if payload.get("schema_version") != CARD_DOCUMENT_VERSION:
        raise ValueError(f"Unsupported card document version: {payload.get('schema_version')!r}")
    if not isinstance(payload.get("expression"), str):
        raise ValueError("Card expression must be a string")

    values = payload.get("values")
    if not isinstance(values, dict) or set(values) != set(LOGICAL_FIELDS):
        raise ValueError("Card values must contain exactly the four logical fields")
    if any(value is not None and not isinstance(value, str) for value in values.values()):
        raise ValueError("Logical field values must be strings or null")

    for key in ("media", "obsolete_media", "issues", "tags"):
        if not isinstance(payload.get(key), list):
            raise ValueError(f"Card {key} must be a list")
    if any(
        not isinstance(item, dict)
        or not isinstance(item.get("filename"), str)
        or not isinstance(item.get("data_base64"), str)
        for item in payload["media"]
    ):
        raise ValueError("Every media item requires string filename and data_base64 values")
    if any(not isinstance(value, str) for key in ("obsolete_media", "issues", "tags") for value in payload[key]):
        raise ValueError("Card list values must be strings")

    provenance = payload.get("provenance")
    if not isinstance(provenance, dict):
        raise ValueError("Card provenance must be an object")
    for field, entries in provenance.items():
        if not isinstance(field, str) or not isinstance(entries, list):
            raise ValueError("Provenance fields must map to lists")
        for entry in entries:
            confidence = entry.get("confidence_percent") if isinstance(entry, dict) else None
            if (
                not isinstance(entry, dict)
                or entry.get("source") not in PROVENANCE_SOURCES
                or not isinstance(entry.get("label"), str)
                or confidence is not None
                and (not isinstance(confidence, int) or isinstance(confidence, bool) or not 0 <= confidence <= 100)
            ):
                raise ValueError(f"Invalid provenance entry for {field}")


def export_batch_job(job: dict[str, Any], items: list[dict[str, Any]]) -> dict[str, Any]:
    """Export a Python batch row and its items as a lossless contract v1.

    SQLite rows have acquired diagnostic columns over time.  Fields not yet
    understood by the native job repository are retained in ``extensions``;
    importing the contract restores them verbatim instead of silently dropping
    recovery data during the staged migration.
    """
    exported_job = {name: job.get(name) for name in _BATCH_JOB_FIELDS if name in job}
    exported_job["extensions"] = _extensions(job, _BATCH_JOB_FIELDS)
    exported_items = []
    for item in items:
        exported = {name: item.get(name) for name in _BATCH_ITEM_FIELDS if name in item}
        artifact_path = item.get("artifact_path")
        if artifact_path:
            artifact_extensions = item.get("artifact_extensions", {})
            if not isinstance(artifact_extensions, dict):
                raise ValueError("Batch artifact extensions must be an object")
            exported["artifact"] = {
                "reference": artifact_path,
                "extensions": dict(artifact_extensions),
            }
        else:
            exported["artifact"] = None
        exported["extensions"] = _extensions(item, _BATCH_ITEM_FIELDS + _BATCH_ITEM_AUXILIARY_FIELDS)
        exported_items.append(exported)
    payload = {
        "schema_version": BATCH_JOB_VERSION,
        "job": exported_job,
        "items": exported_items,
    }
    validate_batch_job(payload)
    return payload


def import_batch_job(payload: dict[str, Any]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """Restore Python-compatible batch rows from a versioned native contract."""
    validate_batch_job(payload)
    job = _restore_extensions(payload["job"], _BATCH_JOB_FIELDS)
    items: list[dict[str, Any]] = []
    for contract_item in payload["items"]:
        item = _restore_extensions(contract_item, _BATCH_ITEM_FIELDS)
        artifact = contract_item.get("artifact")
        if artifact:
            item["artifact_path"] = artifact["reference"]
            item["artifact_extensions"] = dict(artifact.get("extensions", {}))
        items.append(item)
    return job, items


def validate_batch_job(payload: dict[str, Any]) -> None:
    if not isinstance(payload, dict) or payload.get("schema_version") != BATCH_JOB_VERSION:
        raise ValueError(f"Unsupported batch job version: {payload.get('schema_version') if isinstance(payload, dict) else None!r}")
    job = payload.get("job")
    items = payload.get("items")
    if not isinstance(job, dict) or not isinstance(items, list):
        raise ValueError("Batch job requires object job and list items")
    if not isinstance(job.get("id"), str) or not isinstance(job.get("deck_key"), str):
        raise ValueError("Batch job requires string id and deck_key")
    if not isinstance(job.get("settings", {}), dict):
        raise ValueError("Batch job settings must be an object")
    _restore_extensions(job, _BATCH_JOB_FIELDS)
    for item in items:
        if not isinstance(item, dict):
            raise ValueError("Batch items must be objects")
        if not isinstance(item.get("note_id"), int) or isinstance(item["note_id"], bool):
            raise ValueError("Batch items require integer note_id")
        if not isinstance(item.get("word"), str):
            raise ValueError("Batch items require string word")
        artifact = item.get("artifact")
        if artifact is not None and (
            not isinstance(artifact, dict)
            or not isinstance(artifact.get("reference"), str)
            or not isinstance(artifact.get("extensions", {}), dict)
        ):
            raise ValueError("Batch artifact must contain a string reference")
        _restore_extensions(item, _BATCH_ITEM_FIELDS)


def export_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    """Export a SnapshotManager record without losing historical fields."""
    exported = {name: snapshot.get(name) for name in _SNAPSHOT_FIELDS if name in snapshot}
    original = snapshot.get("original_note")
    if original is None:
        exported["original_note"] = None
    elif isinstance(original, dict):
        note = {name: original.get(name) for name in _SNAPSHOT_NOTE_FIELDS if name in original}
        note["extensions"] = _extensions(original, _SNAPSHOT_NOTE_FIELDS)
        exported["original_note"] = note
    else:
        raise ValueError("Snapshot original_note must be an object or null")
    exported["extensions"] = _extensions(snapshot, _SNAPSHOT_FIELDS + ("original_note",))
    payload = {"schema_version": SNAPSHOT_VERSION, "snapshot": exported}
    validate_snapshot(payload)
    return payload


def import_snapshot(payload: dict[str, Any]) -> dict[str, Any]:
    """Restore a SnapshotManager-compatible record from contract v1."""
    validate_snapshot(payload)
    snapshot = _restore_extensions(payload["snapshot"], _SNAPSHOT_FIELDS)
    original = payload["snapshot"].get("original_note")
    if original is not None:
        snapshot["original_note"] = _restore_extensions(original, _SNAPSHOT_NOTE_FIELDS)
    else:
        snapshot["original_note"] = None
    return snapshot


def validate_snapshot(payload: dict[str, Any]) -> None:
    if not isinstance(payload, dict) or payload.get("schema_version") != SNAPSHOT_VERSION:
        raise ValueError(f"Unsupported snapshot version: {payload.get('schema_version') if isinstance(payload, dict) else None!r}")
    snapshot = payload.get("snapshot")
    if not isinstance(snapshot, dict):
        raise ValueError("Snapshot requires an object record")
    if not isinstance(snapshot.get("id"), str) or not isinstance(snapshot.get("word"), str):
        raise ValueError("Snapshot requires string id and word")
    media_before = snapshot.get("media_before", {})
    if not isinstance(media_before, dict) or any(
        not isinstance(name, str) or value is not None and not isinstance(value, str)
        for name, value in media_before.items()
    ):
        raise ValueError("Snapshot media_before must map filenames to strings or null")
    original = snapshot.get("original_note")
    if original is not None:
        if not isinstance(original, dict):
            raise ValueError("Snapshot original_note must be an object or null")
        note_id = original.get("note_id")
        if note_id is not None and (not isinstance(note_id, int) or isinstance(note_id, bool)):
            raise ValueError("Snapshot note_id must be an integer or null")
        _restore_extensions(original, _SNAPSHOT_NOTE_FIELDS)
    _restore_extensions(snapshot, _SNAPSHOT_FIELDS)
