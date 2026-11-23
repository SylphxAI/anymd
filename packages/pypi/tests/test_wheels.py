"""Test the anymd payload in a wheel from the pinned release builder."""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile
from email.parser import BytesParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
PACKAGE = ROOT / "packages/pypi"


def fixture():
    """Use the release wheel writer and inputs with Verify's current native binary."""
    spec = importlib.util.spec_from_file_location("pypi_wheels", ROOT / ".wheel-kit/scripts/pypi-wheels.py")
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    temp = Path(os.environ["RUNNER_TEMP"])
    inputs = json.loads((temp / "wheel-config.json").read_text())
    os.environ.update({f"WHEELS_{key.upper().replace('-', '_')}": value for key, value in inputs.items()})
    name, version = inputs["name"], "0.0.0"
    directory = inputs["python-package-dir"]
    files = {p.relative_to(ROOT).as_posix(): p.read_bytes() for p in (ROOT / directory / name).rglob("*.py")}
    out = Path(tempfile.mkdtemp(prefix="anymd-wheel-", dir=temp))
    return builder.write_wheel(
        out, name, version, builder.PLATFORMS["linux-x64-gnu"], name,
        (ROOT / "target/release/anymd").read_bytes(),
        builder.metadata(name, version, (ROOT / inputs["readme"]).read_bytes()),
        (ROOT / "LICENSE").read_bytes(), builder.package_files(files, directory, name),
    )


@unittest.skipUnless(os.environ.get("ANYMD_WHEEL"), "set ANYMD_WHEEL to a wheel built by Verify")
class WheelTests(unittest.TestCase):
    def test_anymd_api_typing_marker_and_optional_dependencies(self):
        with zipfile.ZipFile(os.environ["ANYMD_WHEEL"]) as wheel:
            for module in ("__init__.py", "_api.py", "langchain.py", "llamaindex.py"):
                path = "anymd/" + module
                self.assertEqual(wheel.read(path), (PACKAGE / path).read_bytes())
            self.assertIn("anymd/py.typed", wheel.namelist())
            info = next(path for path in wheel.namelist() if path.endswith(".dist-info/METADATA"))
            metadata = BytesParser().parsebytes(wheel.read(info))
            self.assertEqual(metadata["Requires-Python"], ">=3.8")
            self.assertEqual(metadata.get_all("Provides-Extra"), ["langchain", "llamaindex"])
            self.assertEqual(metadata.get_all("Requires-Dist"), [
                'langchain-core>=0.3,<2; extra == "langchain"',
                'llama-index-core>=0.12,<1; extra == "llamaindex"',
            ])

    def test_user_install_needs_neither_user_scripts_on_path_nor_a_fallback_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            env = dict(os.environ, PYTHONUSERBASE=str(out / "user"))
            for key in ("PYTHONPATH", "PYTHONNOUSERSITE", "ANYMD_BIN"):
                env.pop(key, None)
            subprocess.run([sys.executable, "-m", "venv", "--system-site-packages", str(out / "venv")], check=True)
            python = out / "venv/bin/python"
            subprocess.run([str(python), "-m", "pip", "install", "--user", "--no-deps", "--no-index",
                            "--ignore-installed", str(Path(env["ANYMD_WHEEL"]).resolve())], env=env, check=True)
            code = '''
import sys
from pathlib import Path
from anymd import convert
from anymd._api import _binary
assert Path(_binary(None)).is_relative_to(sys.argv[2])
assert "<!-- page 1 -->" in convert(sys.argv[1]).text
'''
            competing = out / "venv/bin/anymd"
            competing.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
            competing.chmod(0o755)
            for path in ("", str(competing.parent)):
                with self.subTest(path=path):
                    subprocess.run([str(python), "-c", code, str(ROOT / "test/fixtures/sample.pdf"), str(out / "user")],
                                   env=dict(env, PATH=path), cwd=out, check=True)


if __name__ == "__main__":
    if sys.argv[1:] == ["--fixture"]:
        print(fixture())
    else:
        unittest.main()
