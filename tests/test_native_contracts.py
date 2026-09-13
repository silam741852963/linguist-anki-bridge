import json
from pathlib import Path
import subprocess
import sys

import pytest

from linguist_anki_bridge.contracts import (
    export_batch_job,
    export_card_document,
    export_snapshot,
    import_batch_job,
    import_card_document,
    import_snapshot,
    validate_batch_job,
    validate_card_document,
    validate_snapshot,
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
    "grammar": True,
    "dictionary-preserve": True,
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


def test_batch_contract_round_trips_python_rows_and_unknown_history():
    fixture = json.loads((FIXTURE.parent / "batch-job.v1.json").read_text(encoding="utf-8"))
    job, items = import_batch_job(fixture)
    assert job["legacy_priority"] == "overnight"
    assert items[0]["artifact_path"] == "jobs_artifacts/batch-1/1.json"
    assert items[0]["legacy_stage"] == "ocr-complete"
    assert export_batch_job(job, items) == fixture


def test_snapshot_contract_round_trips_python_record_and_unknown_history():
    fixture = json.loads((FIXTURE.parent / "snapshot.v1.json").read_text(encoding="utf-8"))
    snapshot = import_snapshot(fixture)
    assert snapshot["legacy_sync_marker"] == "v0"
    assert snapshot["original_note"]["legacy_guid"] == "note-guid-42"
    assert export_snapshot(snapshot) == fixture


def test_persistence_contracts_reject_version_and_extension_collisions():
    batch = json.loads((FIXTURE.parent / "batch-job.v1.json").read_text(encoding="utf-8"))
    batch["schema_version"] = 2
    with pytest.raises(ValueError, match="Unsupported"):
        validate_batch_job(batch)

    snapshot = json.loads((FIXTURE.parent / "snapshot.v1.json").read_text(encoding="utf-8"))
    snapshot["snapshot"]["extensions"]["word"] = "not allowed"
    with pytest.raises(ValueError, match="cannot replace"):
        validate_snapshot(snapshot)
