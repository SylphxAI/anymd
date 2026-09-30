"""No-compile regression tests for the registry payload guard."""
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

spec = importlib.util.spec_from_file_location(
    "fork_check", Path(__file__).with_name("check-vendored-crates.py")
)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def archive(files):
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode="w:gz") as tar:
        for name, content in files.items():
            info = tarfile.TarInfo(f"fork-1.0.0/{name}")
            info.size = len(content)
            tar.addfile(info, io.BytesIO(content))
    return data.getvalue()


class ForkCheckTests(unittest.TestCase):
    def run_check(self, local, remote, status=200, pin="1.0.0"):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "vendor/fork").mkdir(parents=True)
            (root / "vendor/fork/Cargo.toml").write_text(
                '[package]\nname = "fork"\nversion = "1.0.0"\n'
            )
            (root / "Cargo.toml").write_text(
                '[workspace.dependencies]\n'
                f'fork = {{ package = "fork", version = "{pin}" }}\n'
            )

            def fetch(request, timeout):
                if status != 200:
                    raise urllib.error.HTTPError(request.full_url, status, "test", {}, None)
                return io.BytesIO(archive(remote) if request.full_url.endswith(".crate") else b'{}')

            def package(command, **kwargs):
                target = Path(command[command.index("--target-dir") + 1]) / "package"
                target.mkdir()
                (target / "fork-1.0.0.crate").write_bytes(archive(local))
                self.assertIn("--no-verify", command)
                self.assertIn("--locked", command)

            with patch.object(checker, "ROOT", root), patch.object(checker, "FORKS", ("vendor/fork",)), \
                    patch.object(checker.urllib.request, "urlopen", side_effect=fetch), \
                    patch.object(checker.subprocess, "run", side_effect=package) as cargo:
                result = checker.check()
                self.assertEqual(cargo.call_count, 0 if status == 404 else 1)
                return result

    def test_identical_payload_ignores_generated_files(self):
        self.assertEqual(self.run_check(
            {"src/lib.rs": b"same", "Cargo.lock": b"new", "Cargo.toml": b'[package]\nname="fork"'},
            {"src/lib.rs": b"same", ".cargo_vcs_info.json": b"old", "Cargo.toml": b'[package]\nname = "fork"'},
        ), 0)

    def test_changed_added_and_removed_sources_fail(self):
        for local in ({"src/lib.rs": b"new"}, {"src/added.rs": b"new"}, {}):
            with self.subTest(local=local):
                self.assertEqual(self.run_check(local, {"src/lib.rs": b"old"}), 1)

    def test_changed_dependency_fails(self):
        self.assertEqual(self.run_check(
            {"Cargo.toml": b'[dependencies]\nparser="2"'},
            {"Cargo.toml": b'[dependencies]\nparser="1"'},
        ), 1)

    def test_new_version_is_allowed(self):
        self.assertEqual(self.run_check({}, {}, status=404), 0)

    def test_registry_failures_are_not_new_versions(self):
        for status in (403, 429, 500):
            with self.subTest(status=status), self.assertRaises(urllib.error.HTTPError):
                self.run_check({}, {}, status=status)

    def test_workspace_pin_must_match(self):
        with self.assertRaisesRegex(RuntimeError, "workspace pin"):
            self.run_check({}, {}, pin="1.0.1")


if __name__ == "__main__":
    unittest.main()
