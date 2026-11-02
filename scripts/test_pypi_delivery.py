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

ROOT = Path(__file__).resolve().parents[1]


def job(workflow, name):
    match = re.search(rf"^  {name}:\n(.*?)(?=^  [\w-]+:|\Z)", workflow, re.M | re.S)
    return match.group(1) if match else ""


def eligible(gates, result, built, cancelled, skipped_ancestor):
    """Evaluate this guard's small AND-only grammar plus documented default.

    This is a contract model, not evidence of a live scheduler outcome. The
    live counterexample is run 41438511024: wheels succeeded with built=true
    after skipped release jobs, but its publisher was skipped.
    """
    terms = {
        "always()": True,
        "!cancelled()": not cancelled,
        "needs.wheels.result == 'success'": result == "success",
        "needs.wheels.outputs.built == 'true'": built == "true",
    }
    has_status = any(gate in ("always()", "!cancelled()") for gate in gates)
    default_success = not cancelled and not skipped_ancestor and result == "success"
    return (has_status or default_success) and all(terms[gate] for gate in gates)


class PyPIDelivery(unittest.TestCase):
    def setUp(self):
        self.workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.pypi = job(self.workflow, "pypi")
        condition = re.search(r"^    if: (.+)$", self.pypi, re.M)
        self.assertIsNotNone(condition)
        self.gates = condition.group(1).split(" && ")

    def test_recovered_wheels_override_skipped_ancestor_propagation(self):
        self.assertIn("needs: wheels", self.pypi)
        self.assertIn("always()", self.gates)
        self.assertNotIn("success()", self.gates)
        self.assertNotIn("workflow_dispatch", self.pypi)
        self.assertRegex(self.workflow, r"push:\n    branches: \[main\]")

    def test_historical_skip_is_a_counterexample_to_the_old_implicit_success(self):
        old = ["needs.wheels.outputs.built == 'true'"]
        recovered = ("success", "true", False, True)
        self.assertFalse(eligible(old, *recovered))
        self.assertTrue(eligible(self.gates, *recovered))

    def test_success_failure_cancellation_and_noop_matrix(self):
        for case in itertools.product(
                ("success", "failure", "cancelled", "skipped"),
                ("true", "false", ""), (False, True), (False, True)):
            result, built, cancelled, _ = case
            expected = result == "success" and built == "true" and not cancelled
            with self.subTest(case=case):
                self.assertEqual(eligible(self.gates, *case), expected)

    def test_failed_wheels_cannot_publish_even_if_the_build_emitted_true(self):
        self.assertIn("needs.wheels.result == 'success'", self.gates)

    def test_cancellation_cannot_start_publication(self):
        self.assertIn("!cancelled()", self.gates)

    def test_complete_version_noop_does_not_start_publication(self):
        self.assertIn("needs.wheels.outputs.built == 'true'", self.gates)
        self.assertEqual(len(self.gates), 4)

    def test_exact_caller_owned_publisher_fails_visibly_without_a_token_fallback(self):
        self.assertIn("environment: pypi", self.pypi)
        self.assertIn("id-token: write", self.pypi)
        self.assertIn("name: wheels", self.pypi)
        self.assertIn("pypa/gh-action-pypi-publish@", self.pypi)
        self.assertIn("packages-dir: dist", self.pypi)
        self.assertIn("skip-existing: true", self.pypi)
        self.assertEqual(len(re.findall(r"^    if:", self.pypi, re.M)), 1)
        for hidden_failure in ("continue-on-error", "secrets.", "password:", "username:"):
            self.assertNotIn(hidden_failure, self.pypi)


if __name__ == "__main__":
    unittest.main()
