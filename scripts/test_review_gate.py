"""Repository contract for the SHA-pinned merge-group review gate."""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ReviewGateTest(unittest.TestCase):
    def test_scope_is_data_and_missing_enforcement_waits_for_owner(self):
        cfg = json.loads((ROOT / ".github/review-stamp.json").read_text())
        self.assertEqual(cfg["context"], "ops-security/review")
        self.assertFalse(cfg["enforceMissing"])
        self.assertEqual(cfg["requireStampPaths"], [".github/workflows/", ".github/actions/", ".github/review-stamp.json"])
        self.assertNotIn("trustedCreatorIds", cfg)

    def test_group_gate_is_pinned_and_in_required_aggregate(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("uses: SylphxAI/.github/.github/actions/review-stamp-gate@fa31ad44ef1c072e0e88245dfc468573f6ca19b8", workflow)
        self.assertIn("trusted-creator-ids: '8020099'", workflow)
        # Parse job boundaries, not the indentation of its nested steps.
        job = re.search(r"(?ms)^  review-stamp:\n(.*?)(?=^  [a-z][a-z-]*:|\Z)", workflow)[1]
        self.assertIn("if: github.event_name == 'merge_group'", job)
        self.assertIn("fetch-depth: 0", job)
        self.assertIn("pull-requests: read", job)
        self.assertNotIn("continue-on-error", job)
        aggregate = re.search(r"(?ms)^  ci-ok:\n(.*?)(?=^  [a-z][a-z-]*:|\Z)", workflow)[1]
        self.assertRegex(aggregate, r"needs: \[[^\n]*review-stamp")
        self.assertIn("if: always()", aggregate)
