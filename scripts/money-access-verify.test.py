import contextlib
import importlib.util
import io
import os
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("verify", Path(__file__).with_name("money-access-verify.py"))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class AccessVerification(unittest.TestCase):
    def test_refusal_diagnostic_excludes_arbitrary_response_content(self):
        problem = {"detail": "The key was refused (key_not_yet_propagated). private-fixture"}
        self.assertEqual(verify.refusal_reason(problem), "key_not_yet_propagated")
        self.assertEqual(verify.refusal_reason({"message": "private-fixture"}), "unclassified")

    def run_verification(self, *, changed_principal=False, refused=False, escape=False):
        home = "orgs/example/projects/project/envs/preview"
        env = {"ACCESS_NAME": "catalogue-preview", "EXPECTED_ENVIRONMENT": home,
               "OTHER_ENVIRONMENT": home.replace("preview", "production"),
               "EXPECTED_SCOPES": "billing:read,billing:write", "GITHUB_REF": "refs/heads/main",
               "EXPECTED_REF": "refs/heads/main", "ACTIONS_ID_TOKEN_REQUEST_URL": "https://oidc.invalid/token?x=1",
               "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "runner-credential", "OTHER_REPOSITORY_DECLARATION": "other",
               "EXPECT_REFUSAL": str(refused).lower()}
        calls = []
        mints = 0

        def respond(url, token=None, body=None):
            nonlocal mints
            calls.append((url, body))
            if url.startswith("https://oidc.invalid/"):
                return 200, {"value": "jwt-fixture"}, None
            if url == verify.API + "access/github/token":
                if refused or body["name"] == "other":
                    return 403, {}, "denied"
                mints += 1
                return 200, {"api_key": "key-fixture", "expires_at": "later", "key": {
                    "name": home + "/api_keys/" + str(mints),
                    "spec": {"scopes": ["billing:write", "billing:read"]},
                    "status": {"principal": str(mints) if changed_principal else "workload"}}}, "issued"
            if url == verify.API + home + "/price_catalogs/default":
                return 200, {}, "own"
            return (200 if escape else 404), {}, "other"

        with patch.dict(os.environ, env), patch.object(verify, "request", side_effect=respond), \
                patch.object(verify.time, "sleep"), contextlib.redirect_stdout(io.StringIO()):
            verify.main()
        return calls

    def test_rotations_read_only_and_fenced(self):
        calls = self.run_verification()
        writes = [(url, body) for url, body in calls if body is not None]
        self.assertEqual(len(writes), 3)
        self.assertTrue(all(url == verify.API + "access/github/token" for url, _ in writes))
        self.assertTrue(all(body["name"] in ("catalogue-preview", "other") for _, body in writes))

    def test_changed_principal_fails(self):
        with self.assertRaisesRegex(RuntimeError, "principal changed"):
            self.run_verification(changed_principal=True)

    def test_cross_environment_access_fails(self):
        with self.assertRaisesRegex(RuntimeError, "Cross-environment"):
            self.run_verification(escape=True)

    def test_undeclared_workflow_does_no_money_reads_or_writes(self):
        calls = self.run_verification(refused=True)
        self.assertEqual(len(calls), 2)
        self.assertTrue(calls[-1][0].endswith("access/github/token"))


if __name__ == "__main__":
    unittest.main()
