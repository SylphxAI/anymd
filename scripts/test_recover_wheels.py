"""Offline recovery fixtures: no Rust, network, publisher, or credentials."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import stat
import subprocess
import tarfile
import tempfile
import unittest
import urllib.error
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("recover_wheels", ROOT / "scripts/recover-wheels.py")
recovery = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(recovery)
VERSION = "8.2.0"
SHA = "a" * 40


def archive_bytes(exe):
    stream = io.BytesIO()
    if exe.endswith(".exe"):
        with zipfile.ZipFile(stream, "w") as archive:
            archive.writestr(exe, b"tagged binary")
    else:
        with tarfile.open(fileobj=stream, mode="w:gz") as archive:
            member = tarfile.TarInfo(exe)
            member.size = len(b"tagged binary")
            archive.addfile(member, io.BytesIO(b"tagged binary"))
    return stream.getvalue()


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.out = self.root / "dist"
        (self.root / "packages/anymd").mkdir(parents=True)
        (self.root / "packages/anymd/package.json").write_text(json.dumps({"version": VERSION}))
        (self.root / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{VERSION}"\n')
        (self.root / "scripts").mkdir()
        shutil.copy(ROOT / "scripts/build-wheels.py", self.root / "scripts")
        shutil.copytree(ROOT / "packages/pypi", self.root / "packages/pypi",
                        ignore=shutil.ignore_patterns("__pycache__"))
        (self.root / "LICENSE").write_text("tagged licence and notices")
        self.assets = {}
        for platform, _, exe in recovery.PLATFORMS:
            name = f"anymd-{platform}.{'zip' if exe.endswith('.exe') else 'tar.gz'}"
            self.assets[name] = archive_bytes(exe)
        self.assets["SHA256SUMS"] = "".join(
            f"{hashlib.sha256(data).hexdigest()}  {name}\n" for name, data in self.assets.items()).encode()
        self.release = {"tag_name": f"v{VERSION}", "draft": False, "assets": [
            {"name": name, "digest": f"sha256:{hashlib.sha256(data).hexdigest()}"}
            for name, data in self.assets.items()]}
        self.calls = []
        self.real_subprocess_run = subprocess.run

    def run_command(self, *args, cwd=None):
        self.calls.append(args)
        if args[:2] == ("gh", "api"):
            self.assertEqual(args[2], f"repos/SylphxAI/anymd/releases/tags/v{VERSION}")
            return json.dumps(self.release)
        if args[:3] == ("gh", "release", "download"):
            self.assertEqual(args[3], f"v{VERSION}")
            directory = Path(args[args.index("--dir") + 1])
            name = args[args.index("--pattern") + 1]
            (directory / name).write_bytes(self.assets[name])
        elif args[:3] == ("gh", "attestation", "verify"):
            self.assertEqual(args[args.index("--source-digest") + 1], SHA)
            self.assertEqual(args[args.index("--signer-workflow") + 1],
                             "SylphxAI/anymd/.github/workflows/release.yml")
        elif args[:2] == ("git", "rev-parse"):
            return SHA
        elif args[-1] == "version":
            return f"anymd {VERSION}"
        return ""

    def run_subprocess(self, args, **kwargs):
        if args[:2] == ["git", "archive"]:
            self.assertEqual(args[2], SHA)
            with tarfile.open(fileobj=kwargs["stdout"], mode="w") as archive:
                for name in args[3:]:
                    archive.add(self.root / name, arcname=name)
            return subprocess.CompletedProcess(args, 0)
        return self.real_subprocess_run(args, **kwargs)

    def deliver(self, complete=False):
        with patch.object(recovery, "pypi_complete", return_value=complete), \
             patch.object(recovery, "run", side_effect=self.run_command), \
             patch.object(recovery.subprocess, "run", side_effect=self.run_subprocess):
            return recovery.deliver(self.root, self.artifacts, self.out)

    def test_missing_pypi_recovers_five_tagged_wheels_and_payload(self):
        self.assertTrue(self.deliver())
        self.assertEqual({p.name for p in self.out.iterdir()}, recovery.wheel_names(VERSION))
        for path in self.out.iterdir():
            with zipfile.ZipFile(path) as wheel:
                exe = "anymd.exe" if "win_amd64" in path.name else "anymd"
                self.assertEqual(wheel.read(f"anymd-{VERSION}.data/scripts/{exe}"), b"tagged binary")
                self.assertEqual(wheel.read(f"anymd-{VERSION}.dist-info/licenses/LICENSE"),
                                 b"tagged licence and notices")
                self.assertIn("anymd/__init__.py", wheel.namelist())
                self.assertIn("anymd/py.typed", wheel.namelist())
                self.assertIn(f"Version: {VERSION}", wheel.read(f"anymd-{VERSION}.dist-info/METADATA").decode())
        self.assertEqual(sum(call[:3] == ("gh", "attestation", "verify") for call in self.calls), 5)

    def test_complete_pypi_is_noop(self):
        self.assertFalse(self.deliver(complete=True))
        self.assertFalse(self.calls)
        self.assertFalse(self.out.exists())

    def test_fresh_native_delivery_does_not_probe_or_recover(self):
        for platform, _, exe in recovery.PLATFORMS:
            path = self.artifacts / f"native-{platform}" / exe
            path.parent.mkdir(parents=True)
            path.write_bytes(b"fresh binary")
        with patch.object(recovery, "pypi_complete", side_effect=AssertionError("must not probe")), \
             patch.object(recovery, "run", side_effect=self.run_command):
            self.assertTrue(recovery.deliver(self.root, self.artifacts, self.out))
        self.assertFalse(any(call[0] == "gh" for call in self.calls))
        self.assertEqual({p.name for p in self.out.iterdir()}, recovery.wheel_names(VERSION))

    def test_downloaded_native_modes_are_restored_without_changing_bytes(self):
        binaries = []
        for platform, _, exe in recovery.PLATFORMS:
            path = self.artifacts / f"native-{platform}" / exe
            path.parent.mkdir(parents=True)
            path.write_bytes(b"unchanged published binary")
            path.chmod(0o644)
            binaries.append(path)
        original = self.run_command
        def require_executable(*args, **kwargs):
            if args[-1] == "version":
                for binary in binaries:
                    self.assertEqual(stat.S_IMODE(binary.stat().st_mode), 0o755)
                    self.assertEqual(binary.read_bytes(), b"unchanged published binary")
            return original(*args, **kwargs)
        with patch.object(recovery, "pypi_complete", side_effect=AssertionError("must not probe")), \
             patch.object(recovery, "run", side_effect=require_executable):
            self.assertTrue(recovery.deliver(self.root, self.artifacts, self.out))
        self.assertFalse(any(call[0] == "gh" for call in self.calls))

    def test_partial_native_matrix_fails_without_release_fallback(self):
        path = self.artifacts / "native-linux-x64-gnu/anymd"
        path.parent.mkdir(parents=True)
        path.write_bytes(b"binary")
        with self.assertRaisesRegex(ValueError, "incomplete native"):
            self.deliver()
        self.assertFalse(self.calls)

    def test_empty_native_is_not_masked_by_release_fallback(self):
        path = self.artifacts / "native-linux-x64-gnu/anymd"
        path.parent.mkdir(parents=True)
        path.touch()
        with self.assertRaisesRegex(ValueError, "incomplete native"):
            self.deliver()
        self.assertFalse(self.calls)

    def test_missing_each_release_target_fails(self):
        original = self.release["assets"]
        for asset in original:
            with self.subTest(asset=asset["name"]):
                self.release["assets"] = [item for item in original if item != asset]
                with self.assertRaisesRegex(ValueError, "missing release target"):
                    self.deliver()
        self.release["assets"] = original

    def test_wrong_release_tag_fails(self):
        self.release["tag_name"] = "v0.0.0"
        with self.assertRaisesRegex(ValueError, "wrong or draft"):
            self.deliver()

    def test_wrong_tagged_source_version_fails(self):
        (self.root / "packages/anymd/package.json").write_text(json.dumps({"version": "0.0.0"}))
        with patch.object(recovery, "run", side_effect=self.run_command), \
             patch.object(recovery.subprocess, "run", side_effect=self.run_subprocess):
            with self.assertRaisesRegex(ValueError, "tagged source version"):
                recovery.recover(self.root, VERSION, self.artifacts, self.root / "work")

    def test_wrong_tagged_cargo_version_fails(self):
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.0.0"\n')
        with self.assertRaisesRegex(ValueError, "tagged Cargo version"):
            self.deliver()

    def test_only_tagged_api_and_licence_are_shipped(self):
        original = self.run_subprocess
        api = self.root / "packages/pypi/anymd/__init__.py"
        api.write_text("# tagged API\n")
        def different_main_payload(args, **kwargs):
            result = original(args, **kwargs)
            if args[:2] == ["git", "archive"]:
                (self.root / "LICENSE").write_text("new main notices")
                api.write_text("# newer main API must never ship\n")
            return result
        with patch.object(recovery, "pypi_complete", return_value=False), \
             patch.object(recovery, "run", side_effect=self.run_command), \
             patch.object(recovery.subprocess, "run", side_effect=different_main_payload):
            self.assertTrue(recovery.deliver(self.root, self.artifacts, self.out))
        for path in self.out.iterdir():
            with zipfile.ZipFile(path) as wheel:
                self.assertEqual(wheel.read("anymd/__init__.py"), b"# tagged API\n")
                self.assertEqual(wheel.read(f"anymd-{VERSION}.dist-info/licenses/LICENSE"),
                                 b"tagged licence and notices")

    def test_historical_tag_without_api_requires_new_release(self):
        shutil.rmtree(self.root / "packages/pypi/anymd")
        with self.assertRaisesRegex(ValueError, "new release containing the API is required"):
            self.deliver()
        self.assertFalse(self.out.exists())

    def test_archive_missing_binary_or_symlink_fails(self):
        path = self.root / "bad.tar.gz"
        destination = self.root / "binary"
        for name, kind in [("wrong-target", tarfile.REGTYPE), ("anymd", tarfile.SYMTYPE)]:
            with tarfile.open(path, "w:gz") as archive:
                member = tarfile.TarInfo(name)
                member.type = kind
                member.linkname = "/old/global/anymd"
                archive.addfile(member)
            with self.subTest(name=name, kind=kind), self.assertRaisesRegex(ValueError, "missing or duplicate"):
                recovery.extract_binary(path, "anymd", destination)
        self.assertFalse(destination.exists())

    def test_corrupt_asset_fails(self):
        name = "anymd-linux-x64-gnu.tar.gz"
        self.assets[name] += b"corruption"
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.deliver()

    def test_checksum_mismatch_fails(self):
        self.assets["SHA256SUMS"] = self.assets["SHA256SUMS"].replace(b"anymd-linux-x64-gnu", b"wrong-name")
        for asset in self.release["assets"]:
            if asset["name"] == "SHA256SUMS":
                asset["digest"] = "sha256:" + hashlib.sha256(self.assets["SHA256SUMS"]).hexdigest()
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.deliver()

    def test_binary_version_mismatch_fails(self):
        real = self.run_command
        def wrong_version(*args, **kwargs):
            return "anymd 0.0.0" if args[-1] == "version" else real(*args, **kwargs)
        with patch.object(recovery, "pypi_complete", return_value=False), \
             patch.object(recovery, "run", side_effect=wrong_version), \
             patch.object(recovery.subprocess, "run", side_effect=self.run_subprocess):
            with self.assertRaisesRegex(ValueError, "binary version mismatch"):
                recovery.deliver(self.root, self.artifacts, self.out)

    def test_download_or_attestation_error_fails(self):
        real = self.run_command
        for failing_command in [("gh", "release", "download"), ("gh", "attestation", "verify")]:
            def fail(*args, **kwargs):
                if args[:3] == failing_command:
                    raise subprocess.CalledProcessError(1, args)
                return real(*args, **kwargs)
            with self.subTest(command=failing_command), \
                 patch.object(recovery, "pypi_complete", return_value=False), \
                 patch.object(recovery, "run", side_effect=fail), \
                 patch.object(recovery.subprocess, "run", side_effect=self.run_subprocess):
                with self.assertRaises(subprocess.CalledProcessError):
                    recovery.deliver(self.root, self.artifacts, self.out)


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
                "source": {"repository": recovery.REPO, "commit": SHA}}))

    def test_original_binary_source_is_independent_of_packaging_checkout(self):
        self.assertEqual(recovery.image_source(self.artifacts, VERSION), SHA)

    def test_mixed_native_sources_fail(self):
        path = self.artifacts / "native-linux-arm64-gnu/identity.json"
        identity = json.loads(path.read_text())
        identity["source"]["commit"] = "b" * 40
        path.write_text(json.dumps(identity))
        with self.assertRaisesRegex(ValueError, "mixed image native sources"):
            recovery.image_source(self.artifacts, VERSION)

    def test_changed_native_bytes_fail(self):
        (self.artifacts / "native-linux-x64-gnu/anymd").write_bytes(b"different native")
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            recovery.image_source(self.artifacts, VERSION)

    def test_wrong_native_version_fails(self):
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            recovery.image_source(self.artifacts, "0.0.0")


class ReleaseImageTests(unittest.TestCase):
    def test_image_recovery_separates_packaging_and_tagged_native_sources(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        image = workflow.split("  image:\n", 1)[1]
        self.assertIn("--image-source", image)
        self.assertIn('tag_source=$(git rev-parse "v$v^{commit}")', image)
        self.assertIn('test "$binary_source" = "${tag_source:-$binary_source}"', image)
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


class ProbeTests(unittest.TestCase):
    def probe(self, data):
        with patch.object(recovery.urllib.request, "urlopen") as request:
            request.return_value.__enter__.return_value = io.StringIO(json.dumps(data))
            return recovery.pypi_complete(VERSION)

    def data(self):
        return {"info": {"version": VERSION}, "urls": [
            {"filename": name, "digests": {"sha256": "a" * 64}, "yanked": False}
            for name in recovery.wheel_names(VERSION)]}

    def test_all_present_and_partial(self):
        data = self.data()
        self.assertTrue(self.probe(data))
        data["urls"].pop()
        self.assertFalse(self.probe(data))

    def test_yanked_target_incomplete(self):
        data = self.data()
        data["urls"][0]["yanked"] = True
        self.assertFalse(self.probe(data))

    def test_mismatched_version_or_filename_fails(self):
        for field in ("version", "filename"):
            data = self.data()
            if field == "version":
                data["info"][field] = "0.0.0"
            else:
                data["urls"][0][field] = "anymd-0.0.0-py3-none-win_amd64.whl"
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.probe(data)

    def test_only_404_is_missing(self):
        for code in (404, 403, 429, 500):
            with self.subTest(code=code), patch.object(recovery.urllib.request, "urlopen",
                    side_effect=urllib.error.HTTPError("https://pypi.org", code, "fixture", None, None)):
                if code == 404:
                    self.assertFalse(recovery.pypi_complete(VERSION))
                else:
                    with self.assertRaises(urllib.error.HTTPError):
                        recovery.pypi_complete(VERSION)
        with patch.object(recovery.urllib.request, "urlopen", side_effect=urllib.error.URLError("offline")):
            with self.assertRaises(urllib.error.URLError):
                recovery.pypi_complete(VERSION)


if __name__ == "__main__":
    unittest.main()
