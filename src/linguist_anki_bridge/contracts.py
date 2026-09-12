"""Versioned interchange contracts used during the native migration.

The native code and the Python application deliberately exchange plain JSON
instead of importing one another. Keeping this adapter small lets parity tests
compare both implementations while Python remains the production fallback.
"""

from __future__ import annotations

from typing import Any

from .card_model import CardDocument, MediaAsset


CARD_DOCUMENT_VERSION = 1
LOGICAL_FIELDS = (
    "meaning_image",
    "meaning_text",
    "kanji_construction",
    "audio",
)
PROVENANCE_SOURCES = {"original", "dictionary", "ocr", "generated", "user"}


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
