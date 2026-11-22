"""Workflow contracts for ordinary PyPI delivery (not a scheduler emulator).

GitHub applies an implicit success() to job conditions without a status
function, including skipped ancestors in a dependency chain. Recovery may
succeed after those ancestors skip, so publication must use its direct wheel
result and output instead. The recovery helper's offline tests own the
complete-version no-op and immutable tagged-payload checks.
"""
import itertools
import re
import unittest
from pathlib import Path

from workflow_condition import condition, eligible, job

ROOT = Path(__file__).resolve().parents[1]


def eligible_publisher(gate, result, built, cancelled, skipped_ancestor, visibility="public", ref="refs/heads/main"):
    """Contract model; live skipped-ancestor counterexample: run 41438511024."""
    return eligible(gate, {
        "github.repository_visibility": visibility,
        "github.ref": ref,
        "needs.wheels.result": result,
        "needs.wheels.outputs.built": built,
    }, success=not cancelled and not skipped_ancestor and result == "success", cancelled=cancelled)


class PyPIDelivery(unittest.TestCase):
    def setUp(self):
        self.workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.pypi = job(self.workflow, "pypi")
        self.gate = condition(self.workflow, "pypi")

    def test_recovered_wheels_override_skipped_ancestor_propagation(self):
        self.assertIn("needs: wheels", self.pypi)
        self.assertNotIn("workflow_dispatch", self.pypi)
        self.assertRegex(self.workflow, r"push:\n    branches: \[main\]")
        self.assertTrue(eligible_publisher(self.gate, "success", "true", False, True))

    def test_historical_skip_is_a_counterexample_to_the_old_implicit_success(self):
        old = "needs.wheels.outputs.built == 'true'"
        recovered = ("success", "true", False, True)
        self.assertFalse(eligible_publisher(old, *recovered))
        self.assertTrue(eligible_publisher(self.gate, *recovered))

    def test_success_failure_cancellation_noop_and_visibility_matrix(self):
        for case in itertools.product(
                ("success", "failure", "cancelled", "skipped"),
                ("true", "false", ""), (False, True), (False, True),
                ("public", "private", "internal", ""), ("refs/heads/main", "refs/heads/feature", "refs/tags/v8.5.15")):
            result, built, cancelled, _, visibility, ref = case
            expected = (visibility == "public" and ref == "refs/heads/main" and result == "success"
                        and built == "true" and not cancelled)
            with self.subTest(case=case):
                self.assertEqual(eligible_publisher(self.gate, *case), expected)

    def test_exact_caller_owned_publisher_fails_visibly_without_a_token_fallback(self):
        self.assertIn("environment: pypi", self.pypi)
        self.assertIn("id-token: write", self.pypi)
        self.assertIn("name: wheels", self.pypi)
        self.assertIn("pypa/gh-action-pypi-publish@", self.pypi)
        self.assertIn("packages-dir: dist", self.pypi)
        self.assertIn("skip-existing: true", self.pypi)
        self.assertLess(self.pypi.index("pypa/gh-action-pypi-publish@"), self.pypi.index("--verify-pypi"))
        self.assertEqual(len(re.findall(r"^    if:", self.pypi, re.M)), 1)
        for hidden_failure in ("continue-on-error", "secrets.", "password:", "username:"):
            self.assertNotIn(hidden_failure, self.pypi)


if __name__ == "__main__":
    unittest.main()
