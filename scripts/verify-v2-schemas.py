#!/usr/bin/env python3
"""Validate checked-in v2 fixtures against generated JSON Schema 2020-12 files."""

import json
import random
from copy import deepcopy
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker


ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = ROOT / "contracts" / "v2"
FIXTURES = CONTRACTS / "fixtures"


def validator(name):
    schema = json.loads((CONTRACTS / f"{name}.schema.json").read_text())
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema, format_checker=FormatChecker())


def require_rejected(checker, value, label):
    assert not checker.is_valid(value), f"schema accepted invalid {label}"


def main():
    schemas = list(CONTRACTS.glob("*.schema.json"))
    for path in schemas:
        validator(path.name.removesuffix(".schema.json"))
    fixture_schemas = {
        "vocabulary.json": "learning-document",
        "grammar.json": "learning-document",
        "vocabulary-rich-dictionary.json": "learning-document",
        "native-operation-receipt.json": "native-operation-receipt",
        "resume-binding-decision.json": "resume-binding-decision",
        "gate-evidence-not-run.json": "gate-evidence",
        "capability-report.json": "capability-report",
        "source-task-map.json": "source-task-map",
    }
    for fixture_name, schema_name in fixture_schemas.items():
        value = json.loads((FIXTURES / fixture_name).read_text())
        errors = list(validator(schema_name).iter_errors(value))
        assert not errors, f"{fixture_name}: {errors}"

    document = json.loads((FIXTURES / "vocabulary.json").read_text())
    document_checker = validator("learning-document")
    invalid = deepcopy(document)
    invalid["schema_version"] = 3
    require_rejected(document_checker, invalid, "document version")
    invalid = deepcopy(document)
    invalid["id"] = "not-a-uuid"
    require_rejected(document_checker, invalid, "document UUID")

    receipt = json.loads((FIXTURES / "native-operation-receipt.json").read_text())
    receipt_checker = validator("native-operation-receipt")
    invalid = deepcopy(receipt)
    invalid["schema_version"] = 2
    require_rejected(receipt_checker, invalid, "receipt version")
    invalid = deepcopy(receipt)
    invalid["readback"]["note_ids"] = ["01"]
    require_rejected(receipt_checker, invalid, "noncanonical Anki ID")
    invalid = deepcopy(receipt)
    invalid["readback"]["note_ids"] = ["90071992547409920"]
    require_rejected(receipt_checker, invalid, "oversized Anki ID")
    invalid = deepcopy(receipt)
    invalid["readback"]["note_ids"] = ["9007199254740992"]
    require_rejected(receipt_checker, invalid, "Anki ID just above safe ceiling")
    for number in [1, 999999999999999, 9007199254740990, 9007199254740991]:
        candidate = deepcopy(receipt)
        candidate["readback"]["note_ids"] = [str(number)]
        assert receipt_checker.is_valid(candidate), f"schema rejected valid Anki ID {number}"
    rng = random.Random(0)
    for _ in range(1000):
        number = rng.randrange(0, 10**17)
        candidate = deepcopy(receipt)
        candidate["readback"]["note_ids"] = [str(number)]
        assert receipt_checker.is_valid(candidate) == (1 <= number <= 9007199254740991), number
    invalid = deepcopy(receipt)
    invalid["approved_digest"] = "b" * 64
    require_rejected(receipt_checker, invalid, "unscoped approval digest")
    invalid = deepcopy(receipt)
    invalid["payload_digest"] = "A" * 64
    require_rejected(receipt_checker, invalid, "uppercase payload digest")

    task_map = json.loads((FIXTURES / "source-task-map.json").read_text())
    task_checker = validator("source-task-map")
    invalid = deepcopy(task_map)
    invalid["schema_version"] = 2
    require_rejected(task_checker, invalid, "task-map version")
    invalid = deepcopy(task_map)
    invalid["source_model_digest"] = "C" * 64
    require_rejected(task_checker, invalid, "task-map digest")

    print(f"PASS: {len(schemas)} v2 schemas and {len(fixture_schemas)} fixtures; version, UUID, ID and digest rejection")


if __name__ == "__main__":
    main()
