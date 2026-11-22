"""Main-path admission and crates.io reconciliation, without credentials or writes."""
import itertools
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import unittest

from workflow_condition import condition, eligible, job

ROOT = Path(__file__).resolve().parents[1]
CRATES = ("anymd-oar-ocr-vl", "anymd-adobe-cmap-parser", "anymd-pdf-extract", "anymd-pdf",
          "anymd-formats", "anymd-core", "anymd-ocr-vlm", "anymd")
VERSIONS = dict(zip(CRATES, ("0.9.5", "0.4.2", "0.12.4", *("8.5.15",) * 5)))


class WorkflowTests(unittest.TestCase):
    def test_crates_run_on_main_push_and_dispatch_even_after_skipped_npm_work(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        gate = condition(workflow, "crates")
        self.assertIn("push:\n    branches: [main]\n  workflow_dispatch:", workflow)
        for event, ref, visibility, result, cancelled, implicit_success in itertools.product(
                ("push", "workflow_dispatch"), ("refs/heads/main", "refs/heads/topic", "refs/tags/v8.5.15"),
                ("public", "private"), ("success", "failure", "cancelled", "skipped"), (False, True), (False, True)):
            context = {"github.ref": ref, "github.repository_visibility": visibility,
                       "github.event_name": event, "needs.release.result": result}
            expected = ref == "refs/heads/main" and visibility == "public" and result == "success" and not cancelled
            with self.subTest(context=context, cancelled=cancelled, implicit_success=implicit_success):
                self.assertEqual(eligible(gate, context, cancelled=cancelled, success=implicit_success), expected)

    def test_authentication_and_publish_are_conditional_on_the_plan(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        crates = job(workflow, "crates")
        self.assertIn("environment: crates-io", crates)
        self.assertIn('publish-crates.sh --plan >> "$GITHUB_OUTPUT"', crates)
        self.assertEqual(crates.count("if: steps.crates-plan.outputs.needed == 'true'"), 2)
        self.assertIn("rust-lang/crates-io-auth-action@", crates)
        self.assertNotIn("secrets.", crates)


class ReconcileTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.registry = self.root / "registry.json"
        self.log = self.root / "publish.log"
        (self.root / "mock-credential").write_text(secrets.token_urlsafe(24))
        self.write_registry({})
        (self.root / "metadata.json").write_text(json.dumps({"packages": [
            {"name": crate, "version": version} for crate, version in VERSIONS.items()]}))
        mock = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

args = sys.argv[1:]
root = Path(os.environ["TEST_ROOT"])
registry = root / "registry.json"
state = json.loads(registry.read_text())
if Path(sys.argv[0]).name == "cargo":
    if args == ["metadata", "--no-deps", "--format-version", "1"]:
        print((root / "metadata.json").read_text())
    else:
        assert args[:-1] == ["publish", "--locked", "--registry", "crates-io", "-p"], args
        assert os.environ["CARGO_REGISTRY_TOKEN"] == (root / "mock-credential").read_text()
        crate = args[-1]
        assert state.get(crate, "missing") == "missing", crate
        with (root / "publish.log").open("a") as output:
            print(crate, file=output)
        if os.environ.get("FAIL_PUBLISH") == crate:
            sys.exit(1)
        if not os.environ.get("NO_VISIBILITY"):
            state[crate] = "propagating"
            registry.write_text(json.dumps(state))
else:
    crate, version = args[-1].removeprefix("https://crates.io/api/v1/crates/").split("/")
    versions = {p["name"]: p["version"] for p in json.loads((root / "metadata.json").read_text())["packages"]}
    assert version == versions[crate], args
    status = state.get(crate, "missing")
    if status == "propagating":
        state[crate] = "present"
        registry.write_text(json.dumps(state))
        status = "missing"
    if status == "network-error":
        sys.exit(28)
    if status == "unavailable":
        print(503)
    elif status == "missing":
        print(404)
    else:
        print(200)
        Path(args[args.index("-o") + 1]).write_text(json.dumps({"version": {
            "crate": crate, "num": "9.9.9" if status == "wrong-version" else version,
            "yanked": status == "yanked"}}))
'''
        for name, script in (("cargo", mock), ("curl", mock), ("sleep", "#!/bin/sh\nexit 0\n")):
            tool = self.root / name
            tool.write_text(script)
            tool.chmod(0o755)

    def write_registry(self, state):
        self.registry.write_text(json.dumps(state))

    def invoke(self, *args, authenticated=False, **env):
        environ = dict(os.environ, PATH=f"{self.root}:{os.environ['PATH']}", TEST_ROOT=str(self.root), **env)
        environ.pop("CARGO_REGISTRY_TOKEN", None)
        if authenticated:
            environ["CARGO_REGISTRY_TOKEN"] = (self.root / "mock-credential").read_text()
        return subprocess.run(["bash", str(ROOT / "scripts/publish-crates.sh"), *args], cwd=ROOT,
                              env=environ, text=True, capture_output=True, timeout=15)

    def test_list_does_not_query_registry(self):
        self.write_registry(dict.fromkeys(CRATES, "network-error"))
        result = self.invoke("--list")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [f"{crate} {version}" for crate, version in VERSIONS.items()])
        self.assertFalse(self.log.exists())

    def test_complete_set_needs_no_token_or_publish(self):
        self.write_registry(dict.fromkeys(CRATES, "present"))
        plan = self.invoke("--plan")
        self.assertEqual(plan.returncode, 0, plan.stderr)
        self.assertEqual(plan.stdout, "needed=false\n")
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.log.exists())

    def test_missing_set_without_a_credential_cannot_publish(self):
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("a crates.io trusted-publishing token is required", result.stderr)
        self.assertFalse(self.log.exists())

    def test_partial_release_only_publishes_missing_versions_in_dependency_order(self):
        missing = [CRATES[i] for i in (0, 2, 4, 7)]
        self.write_registry({crate: "present" for crate in CRATES if crate not in missing})
        plan = self.invoke("--plan")
        self.assertEqual(plan.returncode, 0, plan.stderr)
        self.assertEqual(plan.stdout, "needed=true\n")
        self.assertFalse(self.log.exists())
        result = self.invoke(authenticated=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.log.read_text().splitlines(), missing)
        self.assertEqual(result.stdout.count("verified"), len(missing))
        self.assertEqual(self.invoke("--plan").stdout, "needed=false\n")

    def test_bad_probe_does_not_authorize_any_publish(self):
        for status in ("yanked", "unavailable", "wrong-version", "network-error"):
            with self.subTest(status=status):
                self.write_registry({CRATES[-1]: status})
                for args in ((), ("--plan",)):
                    result = self.invoke(*args, authenticated=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, "")
                    self.assertFalse(self.log.exists())

    def test_publish_failure_stays_visible_and_rerun_resumes(self):
        result = self.invoke(authenticated=True, FAIL_PUBLISH=CRATES[1])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.log.read_text().splitlines(), list(CRATES[:2]))
        result = self.invoke(authenticated=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.log.read_text().splitlines(), [*CRATES[:2], *CRATES[1:]])

    def test_successful_upload_without_registry_confirmation_fails(self):
        result = self.invoke(authenticated=True, NO_VISIBILITY="true")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("did not confirm publication", result.stderr)
        self.assertEqual(self.log.read_text().splitlines(), [CRATES[0]])


if __name__ == "__main__":
    unittest.main()
