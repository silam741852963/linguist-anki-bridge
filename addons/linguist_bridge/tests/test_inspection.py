# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from linguist_bridge.inspection import InspectionError, inspect_note_twice


class InspectionTest(unittest.TestCase):
    def test_repeated_reads_detect_source_drift(self):
        with patch("linguist_bridge.inspection.inspect_note", side_effect=[
                {"note_id": "123", "cards": [{"id": "456", "reviews": []}]},
                {"note_id": "123", "cards": [{"id": "456", "reviews": [{"id": 1}]}]}]):
            with self.assertRaisesRegex(InspectionError, "BRIDGE_INSPECT_SOURCE_DRIFT"):
                inspect_note_twice(object(), "123")

    def test_matching_reads_remain_explicitly_non_atomic(self):
        observed = {"note_id": "123", "atomic_snapshot_verified": False}
        with patch("linguist_bridge.inspection.inspect_note", side_effect=[
                observed.copy(), observed.copy()]):
            result = inspect_note_twice(object(), "123")
        self.assertTrue(result["repeated_reads_matched"])
        self.assertFalse(result["atomic_snapshot_verified"])


if __name__ == "__main__":
    unittest.main()
