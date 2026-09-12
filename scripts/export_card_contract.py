#!/usr/bin/env python3
"""Export a deterministic CardDocument v1 fixture from local source data.

The script intentionally has no provider setup and no default destination. It
only runs the existing Python card builder against a supplied JSON document,
then writes a versioned native-contract document to the explicitly named path.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT / "src"))

from linguist_anki_bridge.card_model import build_card_document, map_document_fields
from linguist_anki_bridge.contracts import export_card_document


VALID_MODES = {"modernize", "inject"}


def parse_source(path: Path) -> dict[str, Any]:
    try:
        source = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"Could not read fixture source {path}: {exc}") from exc
    if not isinstance(source, dict):
        raise ValueError("Fixture source must be a JSON object")
    if source.get("mode") not in VALID_MODES:
        raise ValueError("Fixture source mode must be 'modernize' or 'inject'")
    if not isinstance(source.get("language_key"), str) or not source["language_key"]:
        raise ValueError("Fixture source requires a non-empty language_key")
    if not isinstance(source.get("processed_data"), dict):
        raise ValueError("Fixture source requires processed_data object")
    if "provenance" in source and not isinstance(source["provenance"], dict):
        raise ValueError("Fixture source provenance must be an object")
    return source


def build_contract(source: dict[str, Any]) -> dict[str, Any]:
    document = build_card_document(
        source["processed_data"], source["language_key"], source["mode"]
    )
    expected_fields = source.get("expected_mapped_fields")
    if expected_fields is not None:
        mapping = source.get("field_mapping")
        if not isinstance(mapping, dict) or not isinstance(expected_fields, dict):
            raise ValueError("Mapped-field expectations require field_mapping and expected_mapped_fields objects")
        actual_fields = map_document_fields(document, {"fields": mapping})
        if actual_fields != expected_fields:
            raise ValueError(
                "Fixture mapped fields differ from expected_mapped_fields: "
                f"{actual_fields!r} != {expected_fields!r}"
            )
    return export_card_document(document, source.get("provenance"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True, help="fixture source JSON")
    parser.add_argument("--output", type=Path, required=True, help="explicit contract JSON destination")
    arguments = parser.parse_args(argv)

    input_path = arguments.input.resolve()
    output_path = arguments.output.resolve()
    if input_path == output_path:
        parser.error("--input and --output must be different paths")
    if not output_path.parent.is_dir():
        parser.error(f"output directory does not exist: {output_path.parent}")

    try:
        payload = build_contract(parse_source(input_path))
    except ValueError as exc:
        parser.error(str(exc))
    output_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
