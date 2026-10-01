"""Verify API inclusion, optional dependencies and wheel RECORD hashes."""

import base64
import csv
import hashlib
import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile
from email.parser import Parser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location(
    "build_wheels", ROOT / "scripts/build-wheels.py"
)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class WheelTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "linux", "POSIX script fixture and Linux wheel tag")
    def test_user_install_uses_record_binary_without_user_scripts_on_path(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            environment = dict(os.environ, PYTHONUSERBASE=str(out / "user"))
            for key in ("PYTHONPATH", "PYTHONNOUSERSITE", "ANYMD_BIN"):
                environment.pop(key, None)
            # Enable an isolated user site without writing to the desk's Python
            # installation. The wheel binary is an executable fixture, not a build.
            subprocess.run([sys.executable, "-m", "venv", "--system-site-packages", str(out / "venv")], check=True)
            python = out / "venv/bin/python"
            fixture = out / "native"
            fixture.write_text(
                '#!' + str(python) + '\nimport sys\n'
                'print("---\\nsource: " + sys.argv[1] + "\\n---\\n\\nmatching wheel")\n',
                encoding="utf-8",
            )
            wheel = builder.build("0.0.0", "manylinux_2_17_x86_64", fixture, out)
            subprocess.run([str(python), "-m", "pip", "install", "--user", "--no-deps",
                            "--no-index", "--ignore-installed", str(wheel)],
                           env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
            source = out / "source.txt"
            source.write_text("input", encoding="utf-8")
            # Empty PATH proves discovery does not depend on user/bin. The second
            # pass also places a competing executable in default scripts and PATH.
            environment["PATH"] = ""
            code = '''
import sys
from anymd import convert
from anymd._api import _binary
from importlib import metadata
assert str(metadata.distribution('anymd').locate_file('')).startswith(sys.argv[2])
assert _binary(None).startswith(sys.argv[2])
assert convert(sys.argv[1]).text == 'matching wheel\\n'
'''
            for competing in (False, True):
                with self.subTest(competing=competing):
                    if competing:
                        wrong = out / "venv/bin/anymd"
                        wrong.write_text('#!' + str(python) + '\nraise SystemExit("wrong binary")\n', encoding="utf-8")
                        wrong.chmod(0o755)
                        environment["PATH"] = str(wrong.parent)
                    subprocess.run([str(python), "-c", code, str(source), str(out / "user")],
                                   env=environment, cwd=out, check=True)

    def test_platform_wheels_include_api_and_only_optional_deps(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            binary = out / "native"
            binary.write_bytes(b"native fixture")
            for tag, exe in (
                ("manylinux_2_17_x86_64.manylinux2014_x86_64", "anymd"),
                ("win_amd64", "anymd.exe"),
            ):
                with self.subTest(tag=tag):
                    wheel = builder.build("8.2.0", tag, binary, out)
                    original = wheel.read_bytes()
                    self.assertEqual(
                        builder.build("8.2.0", tag, binary, out).read_bytes(), original
                    )
                    with zipfile.ZipFile(wheel) as archive:
                        names = set(archive.namelist())
                        for name in (
                            "__init__.py",
                            "_api.py",
                            "langchain.py",
                            "llamaindex.py",
                            "py.typed",
                        ):
                            self.assertIn("anymd/" + name, names)
                        script = "anymd-8.2.0.data/scripts/" + exe
                        self.assertEqual(archive.read(script), b"native fixture")
                        self.assertEqual(
                            (archive.getinfo(script).external_attr >> 16) & 0o777, 0o755
                        )
                        info = "anymd-8.2.0.dist-info/"
                        metadata = Parser().parsestr(
                            archive.read(info + "METADATA").decode()
                        )
                        self.assertEqual(
                            metadata.get_all("Provides-Extra"),
                            ["langchain", "llamaindex"],
                        )
                        self.assertEqual(len(metadata.get_all("Requires-Dist")), 2)
                        self.assertTrue(
                            all(
                                "extra ==" in requirement
                                for requirement in metadata.get_all("Requires-Dist")
                            )
                        )
                        self.assertEqual(metadata["Requires-Python"], ">=3.8")
                        record = list(
                            csv.reader(
                                io.StringIO(archive.read(info + "RECORD").decode())
                            )
                        )
                        self.assertEqual({row[0] for row in record}, names)
                        for name, digest, size in record:
                            if name.endswith("/RECORD"):
                                self.assertEqual((digest, size), ("", ""))
                                continue
                            data = archive.read(name)
                            expected = (
                                base64.urlsafe_b64encode(hashlib.sha256(data).digest())
                                .rstrip(b"=")
                                .decode()
                            )
                            self.assertEqual(digest, "sha256=" + expected)
                            self.assertEqual(int(size), len(data))


if __name__ == "__main__":
    unittest.main()
