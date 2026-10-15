#!/usr/bin/env python3
"""Unit tests for ensure-trusted-publishers.py (no network)."""

import importlib.util
import io
import pathlib
import unittest
from contextlib import redirect_stderr, redirect_stdout

MODULE = pathlib.Path(__file__).with_name("ensure-trusted-publishers.py")

spec = importlib.util.spec_from_loader(
    "ensure_trusted_publishers",
    importlib.machinery.SourceFileLoader("ensure_trusted_publishers", str(MODULE)),
)
etp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(etp)

CONFIG = {
    "crate": "sylphx",
    "repository_owner": "SylphxAI",
    "repository_name": "cloud",
    "workflow_filename": "release.yml",
    "environment": "crates-io",
}


class FakeRegistry:
    """A crates.io stand-in that records every call."""

    def __init__(self, configs=None, create_status=200, list_status=200):
        self.configs = list(configs or [])
        self.create_status = create_status
        self.list_status = list_status
        self.calls = []

    def __call__(self, method, url, token, payload=None):
        self.calls.append((method, url, payload))
        if method == "GET":
            if self.list_status != 200:
                return self.list_status, {"errors": [{"detail": "no"}]}
            return 200, {"github_configs": list(self.configs)}
        self.configs.append(payload["github_config"])
        return self.create_status, payload


class Ensure(unittest.TestCase):
    def configure(self, registry, crate="sylphx"):
        return etp.ensure(etp.API, "token", crate, "SylphxAI", "cloud", "release.yml", "crates-io")

    def test_creates_a_missing_record(self):
        registry = FakeRegistry()
        original = etp.call
        etp.call = registry
        try:
            result = self.configure(registry)
        finally:
            etp.call = original
        self.assertEqual(result, "created")
        posts = [c for c in registry.calls if c[0] == "POST"]
        self.assertEqual(len(posts), 1)
        self.assertEqual(posts[0][2]["github_config"]["crate"], "sylphx")

    def test_leaves_an_existing_record_alone(self):
        registry = FakeRegistry(configs=[CONFIG])
        original = etp.call
        etp.call = registry
        try:
            result = self.configure(registry)
        finally:
            etp.call = original
        self.assertEqual(result, "present")
        self.assertEqual([c for c in registry.calls if c[0] == "POST"], [])

    def test_a_record_for_another_workflow_is_not_a_match(self):
        other = dict(CONFIG, workflow_filename="other.yml")
        self.assertFalse(etp.matches(other, "sylphx", "SylphxAI", "cloud", "release.yml", "crates-io"))
        environmentless = dict(CONFIG, environment="")
        self.assertFalse(etp.matches(environmentless, "sylphx", "SylphxAI", "cloud", "release.yml", "crates-io"))

    def test_a_failed_create_is_an_error(self):
        registry = FakeRegistry(create_status=422)
        original = etp.call
        etp.call = registry
        try:
            with self.assertRaises(RuntimeError):
                self.configure(registry)
        finally:
            etp.call = original

    def test_an_unreadable_listing_is_an_error(self):
        registry = FakeRegistry(list_status=503)
        original = etp.call
        etp.call = registry
        try:
            with self.assertRaises(RuntimeError):
                self.configure(registry)
        finally:
            etp.call = original


class Main(unittest.TestCase):
    def run_main(self, argv):
        original_call = etp.call
        original_env = etp.os.environ.get("CARGO_REGISTRY_TOKEN")
        registry = FakeRegistry()
        etp.call = registry
        etp.os.environ["CARGO_REGISTRY_TOKEN"] = "token"
        out, err = io.StringIO(), io.StringIO()
        try:
            with redirect_stdout(out), redirect_stderr(err):
                code = etp.main(argv)
        finally:
            etp.call = original_call
            if original_env is None:
                etp.os.environ.pop("CARGO_REGISTRY_TOKEN", None)
            else:
                etp.os.environ["CARGO_REGISTRY_TOKEN"] = original_env
        return code, out.getvalue(), err.getvalue()

    def test_configures_every_named_crate(self):
        code, out, _ = self.run_main([
            "--crate", "sylphx", "--crate", "sylphx-cli",
            "--owner", "SylphxAI", "--repo", "cloud",
            "--workflow", "release.yml", "--environment", "crates-io",
        ])
        self.assertEqual(code, 0)
        self.assertIn("sylphx: trusted publisher created", out)
        self.assertIn("sylphx-cli: trusted publisher created", out)

    def test_no_token_fails_without_calling_the_registry(self):
        original_call = etp.call
        original_env = etp.os.environ.pop("CARGO_REGISTRY_TOKEN", None)
        registry = FakeRegistry()
        etp.call = registry
        err = io.StringIO()
        try:
            with redirect_stderr(err):
                code = etp.main([
                    "--crate", "sylphx", "--owner", "SylphxAI",
                    "--repo", "cloud", "--workflow", "release.yml",
                ])
        finally:
            etp.call = original_call
            if original_env is not None:
                etp.os.environ["CARGO_REGISTRY_TOKEN"] = original_env
        self.assertEqual(code, 1)
        self.assertEqual(registry.calls, [])
        self.assertIn("trusted-publishing scope", err.getvalue())


if __name__ == "__main__":
    unittest.main()
