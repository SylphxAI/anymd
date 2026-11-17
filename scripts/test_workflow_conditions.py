"""Admission contracts for catalogue writes and release attestations.

These matrices model the supported condition subset offline. GitHub job
execution and actionlint remain the authority for scheduler behavior/syntax.
"""
import itertools
import unittest
from pathlib import Path

from workflow_condition import condition, eligible

ROOT = Path(__file__).resolve().parents[1]
VISIBILITIES = ("public", "private", "internal", "")
RESULTS = ("success", "failure", "cancelled", "skipped")


class ConditionModel(unittest.TestCase):
    def test_wrapping_spacing_and_parentheses_do_not_change_admission(self):
        context = {"github.repository_visibility": "public", "inputs.verify_only": False}
        for expression in (
            "github.repository_visibility == 'public' && inputs.verify_only != true",
            "$" + "{{ ( github.repository_visibility=='public' ) && (inputs.verify_only!=true) }}",
        ):
            self.assertTrue(eligible(expression, context))
        self.assertTrue(eligible("always() && !cancelled()", {}, success=False))
        self.assertFalse(eligible("true", {}, success=False))
        self.assertFalse(eligible("always() && !cancelled()", {}, cancelled=True))

    def test_unknown_context_and_unsupported_syntax_fail(self):
        for expression in ("missing.value == 'public'", "unknown()", "true + false", "true[0]"):
            with self.subTest(expression=expression), self.assertRaises((ValueError, SyntaxError)):
                eligible(expression, {})


class MoneyCatalogueConditions(unittest.TestCase):
    def test_offline_prs_writes_and_verify_only_matrix(self):
        workflow = (ROOT / ".github/workflows/money-catalogue.yml").read_text()
        gates = {name: condition(workflow, name) for name in ("test", "preview", "production", "verify")}
        for visibility, event, ref, verify, success in itertools.product(
                VISIBILITIES, ("pull_request", "push", "workflow_dispatch"),
                ("refs/heads/main", "refs/heads/feature"), (True, False, ""), (True, False)):
            context = {"github.repository_visibility": visibility, "github.event_name": event,
                       "github.ref": ref, "inputs.verify_only": verify}
            public = visibility == "public" and success
            write = public and event != "pull_request" and ref == "refs/heads/main" and verify is not True
            read = public and event == "workflow_dispatch" and verify is True
            with self.subTest(context=context, success=success):
                self.assertEqual(eligible(gates["test"], context, success=success), public)
                for name in ("preview", "production"):
                    self.assertEqual(eligible(gates[name], context, success=success), write)
                self.assertEqual(eligible(gates["verify"], context, success=success), read)


class ReleaseTrustConditions(unittest.TestCase):
    def test_attestation_dependency_and_visibility_matrix(self):
        gate = condition((ROOT / ".github/workflows/release.yml").read_text(), "trust")
        for visibility, release, plan, build, companion, cancelled in itertools.product(
                VISIBILITIES, RESULTS, RESULTS, RESULTS, RESULTS, (False, True)):
            context = {"github.repository_visibility": visibility,
                       "needs.release.result": release, "needs.companion-plan.result": plan,
                       "needs.companion-build.result": build, "needs.companion.result": companion}
            expected = (visibility == "public" and not cancelled and release == "success"
                        and plan == "success" and build in ("success", "skipped")
                        and companion in ("success", "skipped"))
            success = all(result == "success" for result in (release, plan, build, companion)) and not cancelled
            with self.subTest(context=context, cancelled=cancelled):
                self.assertEqual(eligible(gate, context, success=success, cancelled=cancelled), expected)


if __name__ == "__main__":
    unittest.main()
