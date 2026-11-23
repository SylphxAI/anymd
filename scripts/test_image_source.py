"""GHCR packaging keeps the original native source and licence."""

import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = "8.5.16"
SHA = "a" * 40
SPEC = importlib.util.spec_from_file_location("image_source", ROOT / "scripts/verify-image-source.py")
verifier = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verifier)


class ImageSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.artifacts = Path(self.temp.name)
        for platform in ("linux-x64-gnu", "linux-arm64-gnu"):
            directory = self.artifacts / f"native-{platform}"
            directory.mkdir()
            binary = b"original immutable native"
            (directory / "anymd").write_bytes(binary)
            (directory / "identity.json").write_text(json.dumps({
                "name": "anymd", "version": VERSION, "platform": platform,
                "sha256": hashlib.sha256(binary).hexdigest(),
                "source": {"repository": verifier.REPO, "commit": SHA}}))

    def test_original_binary_source_is_independent_of_packaging_checkout(self):
        self.assertEqual(verifier.image_source(self.artifacts, VERSION), SHA)

    def test_mixed_native_sources_fail(self):
        path = self.artifacts / "native-linux-arm64-gnu/identity.json"
        identity = json.loads(path.read_text())
        identity["source"]["commit"] = "b" * 40
        path.write_text(json.dumps(identity))
        with self.assertRaisesRegex(ValueError, "mixed image native sources"):
            verifier.image_source(self.artifacts, VERSION)

    def test_changed_native_bytes_fail(self):
        (self.artifacts / "native-linux-x64-gnu/anymd").write_bytes(b"different native")
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            verifier.image_source(self.artifacts, VERSION)

    def test_wrong_native_version_fails(self):
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            verifier.image_source(self.artifacts, "0.0.0")


class ReleaseImageTests(unittest.TestCase):
    def test_image_recovery_separates_packaging_and_tagged_native_sources(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        image = workflow.split("  image:\n", 1)[1]
        self.assertIn('python3 scripts/verify-image-source.py artifacts "$v"', image)
        self.assertIn('tag_source=$(git rev-parse "v$v^{commit}")', image)
        self.assertIn('gh release download "v$v" --dir assets', image)
        self.assertIn('test "$binary_source" = "$tag_source"', image)
        self.assertIn('git show "$binary_source:LICENSE" > LICENSE', image)
        self.assertIn("io.sylphx.native.source=${{ steps.stage.outputs.source }}", image)
        self.assertIn("io.sylphx.packaging.source=${{ github.sha }}", image)
        self.assertIn("file: Dockerfile.release", image)
        self.assertNotIn("ref: ${{ needs.release.outputs.canonical }}", image)

    def test_license_parent_is_created_traversable_before_copy(self):
        dockerfile = (ROOT / "Dockerfile.release").read_text()
        create = "install -d -m 0755 /usr/share/licenses/anymd"
        copy = "COPY --chmod=0644 LICENSE /usr/share/licenses/anymd/LICENSE"
        self.assertIn(create, dockerfile)
        self.assertLess(dockerfile.index(create), dockerfile.index(copy))
        self.assertLess(dockerfile.index(copy), dockerfile.index("USER anymd"))


if __name__ == "__main__":
    unittest.main()
