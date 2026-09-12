import json
from pathlib import Path

import pytest

from linguist_anki_bridge.contracts import (
    export_card_document,
    import_card_document,
    validate_card_document,
)


FIXTURE = Path(__file__).parents[1] / "contracts" / "fixtures" / "card-document.v1.json"


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
