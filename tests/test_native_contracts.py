import json
from pathlib import Path
import subprocess
import sys

import pytest

from linguist_anki_bridge.contracts import (
    export_card_document,
    import_card_document,
    validate_card_document,
)


FIXTURE = Path(__file__).parents[1] / "contracts" / "fixtures" / "card-document.v1.json"
ROOT = FIXTURE.parents[2]
EXPORTER = ROOT / "scripts" / "export_card_contract.py"
SOURCE_DIR = ROOT / "contracts" / "fixture-sources"
GENERATED_FIXTURES = {
    "modernization": True,
    "injection": True,
    "shared-fields": True,
    "media-replacement": False,
    "validation-issues": False,
}


def test_native_fixture_round_trips_through_python_card_model():
    payload = json.loads(FIXTURE.read_text(encoding="utf-8"))
    document = import_card_document(payload)
    exported = export_card_document(document, payload["provenance"])
    assert exported == payload
    assert document.ready


def test_contract_rejects_unknown_version_before_processing():
    payload = json.loads(FIXTURE.read_text(encoding="utf-8"))
    payload["schema_version"] = 2
    with pytest.raises(ValueError, match="Unsupported"):
        validate_card_document(payload)


def test_contract_rejects_out_of_range_confidence():
    payload = json.loads(FIXTURE.read_text(encoding="utf-8"))
    payload["provenance"]["meaning_text"][0]["confidence_percent"] = 101
    with pytest.raises(ValueError, match="provenance"):
        validate_card_document(payload)


@pytest.mark.parametrize("name,ready", GENERATED_FIXTURES.items())
def test_generated_fixture_round_trips_and_has_no_network_dependency(name, ready):
    payload = json.loads((FIXTURE.parent / f"{name}-card.v1.json").read_text(encoding="utf-8"))
    document = import_card_document(payload)
    assert export_card_document(document, payload["provenance"]) == payload
    assert document.ready is ready
    assert "http://" not in json.dumps(payload, ensure_ascii=False)
    assert "https://" not in json.dumps(payload, ensure_ascii=False)


@pytest.mark.parametrize("name", GENERATED_FIXTURES)
def test_exporter_reproduces_each_checked_in_fixture(tmp_path, name):
    output = tmp_path / f"{name}.json"
    completed = subprocess.run(
        [
            sys.executable,
            str(EXPORTER),
            "--input",
            str(SOURCE_DIR / f"{name}.json"),
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 0, completed.stderr
    assert output.read_text(encoding="utf-8") == (
        FIXTURE.parent / f"{name}-card.v1.json"
    ).read_text(encoding="utf-8")


def test_exporter_requires_different_explicit_input_and_output():
    source = SOURCE_DIR / "modernization.json"
    completed = subprocess.run(
        [sys.executable, str(EXPORTER), "--input", str(source), "--output", str(source)],
        check=False,
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 2
    assert "must be different" in completed.stderr
