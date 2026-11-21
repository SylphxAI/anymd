"""Admission contracts for Verify, catalogue writes, and release attestations.

These matrices model the supported condition subset offline. GitHub job
execution and actionlint remain the authority for scheduler behavior/syntax.
"""
import itertools
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from textwrap import dedent

from workflow_condition import condition, eligible, job

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


class VerifyPlan(unittest.TestCase):
    def test_exact_pr_diff_is_required_to_skip_any_lane(self):
        workflow = (ROOT / ".github/workflows/verify.yml").read_text()
        classify = dedent(job(workflow, "plan").split("        run: |\n", 1)[1])
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            output = Path(tmp) / "output"
            repo.mkdir()

            def git(*args, data=None):
                return subprocess.check_output(["git", "-c", "user.name=Test", "-c", "user.email=test@example.com",
                                                *args], cwd=repo, input=data, text=True).strip()

            git("init", "-q")
            base = git("commit-tree", git("mktree", data=""), data="base")
            blob = git("hash-object", "-w", "--stdin", data="changed\n")
            for paths, expected in (
                (["apps/web/web file\n.ts"], "web"), (["crates/anymd/src/cli.rs"], "native"),
                (["packages/pypi/new.py"], "native"), (["apps/web/a.ts", "packages/pypi/a.py"], "mixed"),
                (["crates/anymd-formats/src/docx.rs"], "mixed"), (["sylphx.toml"], "mixed"),
                (["apps/web/a.ts", ".github/workflows/verify.yml"], "full"), (["LICENSE"], "none"), ([], "full"),
            ):
                with self.subTest(paths=paths):
                    git("read-tree", "--empty")
                    for path in paths:
                        git("update-index", "--add", "--cacheinfo", f"100644,{blob},{path}")
                    tree = git("write-tree")
                    head = git("commit-tree", tree, "-p", base, data="head")
                    merge = git("commit-tree", tree, "-p", base, "-p", head, data="test merge")
                    git("update-ref", "HEAD", merge)
                    for event_head, scope in ((head, expected), (base, "full"), ("", "full")):
                        output.write_text("")
                        subprocess.run(["bash", "-e", "-o", "pipefail", "-c", classify], cwd=repo, check=True,
                                       env=dict(os.environ, BASE=base, HEAD=event_head, GITHUB_OUTPUT=str(output)))
                        self.assertEqual(output.read_text(), f"scope={scope}\n")


if __name__ == "__main__":
    unittest.main()
