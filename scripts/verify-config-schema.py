#!/usr/bin/env python3
"""Check the generated normalized-config schema against actual CLI defaults."""
import copy
import json
import subprocess
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "contracts/v2/config-file-normalized.schema.json"


def main():
    schema = json.loads(SCHEMA.read_text())
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    result = subprocess.run(
        ["cargo", "run", "--locked", "-q", "-p", "linguist-cli", "--", "--output", "json", "config", "show", "--defaults"],
        cwd=ROOT, check=True, capture_output=True, text=True, timeout=60,
    )
    defaults = json.loads(result.stdout)["values"]
    assert validator.is_valid(defaults), list(validator.iter_errors(defaults))
    assert len(schema["properties"]) + len(schema["patternProperties"]) == 169

    positive = copy.deepcopy(defaults)
    positive["purposes.japanese_vocab.card_tasks"] = {
        "0": "comprehension", "65535": "production"
    }
    positive["profiles.study.overrides"] = {"llm.model": "reviewed:model"}
    assert validator.is_valid(positive), list(validator.iter_errors(positive))

    invalid = [
        ("unknown key", "invented.setting", True),
        ("config version", "config.version", 3),
        ("integer coercion", "jobs.lease_seconds", "100"),
        ("unknown enum", "dictionary.provider", "invented"),
        ("invalid map ordinal", "purposes.japanese_vocab.card_tasks", {"01": "comprehension"}),
        ("out-of-range map ordinal", "purposes.japanese_vocab.card_tasks", {"65536": "comprehension"}),
        ("unknown field role", "purposes.japanese_vocab.fields", {"invented": "Front"}),
        ("global profile override", "profiles.study.overrides", {"storage.state_dir": "/tmp"}),
    ]
    for label, key, value in invalid:
        candidate = copy.deepcopy(defaults)
        candidate[key] = value
        assert not validator.is_valid(candidate), f"accepted {label}"
    print("PASS: normalized config schema covers 169 registry entries, defaults and closed mappings")


if __name__ == "__main__":
    main()
