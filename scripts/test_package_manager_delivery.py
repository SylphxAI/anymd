"""Keep enabled package channels on the ordinary main delivery path."""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/release.yml"
KIT = "af0ad67cb4b7ea17212cd4cd48640ab8e60e96df"


def job(workflow, name):
    match = re.search(rf"^  {name}:\n(.*?)(?=^  [\w-]+:|\Z)", workflow, re.M | re.S)
    return match.group(1) if match else ""


class PackageManagerDelivery(unittest.TestCase):
    def setUp(self):
        self.workflow = WORKFLOW.read_text()
        self.release = job(self.workflow, "release")
        self.managers = job(self.workflow, "package-managers")

    def test_release_uses_one_immutable_kit_without_the_legacy_secret_path(self):
        self.assertIn(f"release.yml@{KIT}", self.release)
        self.assertIn(f"kit-ref: {KIT}", self.release)
        for obsolete in ("TAP_APP", "secrets:", "homebrew: true", "scoop: true"):
            self.assertNotIn(obsolete, self.release)

    def test_ordinary_main_delivery_runs_caller_owned_authorization(self):
        self.assertRegex(self.workflow, r"push:\n    branches: \[main\]")
        self.assertIn("needs: release", self.managers)
        self.assertIn("if: github.repository_visibility == 'public' && github.ref == 'refs/heads/main'", self.managers)
        self.assertIn("runs-on: ubuntu-latest", self.managers)
        self.assertIn("contents: read", self.managers)
        self.assertIn("id-token: write", self.managers)
        self.assertIn(f".github/actions/package-managers@{KIT}", self.managers)
        for skipping in ("workflow_dispatch", "secrets.", "continue-on-error", "always()"):
            self.assertNotIn(skipping, self.managers)

    def test_version_and_metadata_come_from_the_same_product_manifest(self):
        self.assertIn('require("./packages/anymd/package.json").version', self.managers)
        self.assertIn("version: ${{ steps.version.outputs.value }}", self.managers)
        self.assertIn("package-json: packages/anymd/package.json", self.managers)
        self.assertIn("name: anymd", self.managers)
        self.assertNotIn("8.5.", self.managers)

    def test_channel_description_fits_homebrew_instead_of_the_long_npm_description(self):
        description = re.search(r"^          description: (.+)$", self.managers, re.M)
        self.assertIsNotNone(description)
        value = description.group(1)
        self.assertLessEqual(len(value), 80)
        self.assertTrue(value.isascii())
        self.assertFalse(value.endswith("."))

    def test_both_destinations_use_the_declared_source_identity(self):
        for expected in (
            "ci-name: anymd-package-managers",
            "trust-policy: orgs/org_fs01afbdccgn8xdk1ghj5y47w0vtph/trust_policies/anymd-package-managers",
            "homebrew-tap: SylphxAI/homebrew-tap",
            "scoop-bucket: SylphxAI/scoop-bucket",
            "persist-credentials: false",
        ):
            self.assertIn(expected, self.managers)


if __name__ == "__main__":
    unittest.main()
