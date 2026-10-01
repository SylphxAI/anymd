#!/usr/bin/env python3
"""Build the PyPI platform wheels: one wheel per platform, each carrying the
native anymd binary as a script, so `pip install anymd` and `uvx anymd` run it.

Same layout as ruff and uv: `<name>-<version>.data/scripts/<binary>` is copied
onto PATH at install time. Standard library only, no compiler.

    python3 scripts/build-wheels.py --version 8.1.0 --out dist \
        --binary manylinux_2_17_x86_64.manylinux2014_x86_64=path/to/anymd ...

A platform tag whose name starts with `win` gets `anymd.exe`.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import stat
import sys
import zipfile
from pathlib import Path

NAME = "anymd"
ROOT = Path(__file__).resolve().parents[1]
PYPI = ROOT / "packages" / "pypi"


def digest(data: bytes) -> str:
    raw = hashlib.sha256(data).digest()
    return "sha256=" + base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def metadata(version: str) -> str:
    readme = (PYPI / "README.md").read_text(encoding="utf-8")
    return (
        "Metadata-Version: 2.4\n"
        + f"Name: {NAME}\n"
        + f"Version: {version}\n"
        + "Summary: Any file to clean Markdown for AI agents: PDF, Word, PowerPoint, Excel, EPUB, HTML, images (OCR), audio and video. A fast Rust CLI and MCP server.\n"
        + "Keywords: markdown,pdf,mcp,model-context-protocol,document-conversion,ai-agents\n"
        + "Author-email: Sylphx <contact@sylphx.com>\n"
        + "License-Expression: MIT\n"
        + "License-File: LICENSE\n"
        + "Project-URL: Homepage, https://sylphxai.github.io/anymd/\n"
        + "Project-URL: Source, https://github.com/SylphxAI/anymd\n"
        + "Project-URL: Changelog, https://github.com/SylphxAI/anymd/blob/main/CHANGELOG.md\n"
        + "Classifier: Programming Language :: Rust\n"
        + "Classifier: Topic :: Text Processing :: Markup :: Markdown\n"
        + "Requires-Python: >=3.8\n"
        + "Provides-Extra: langchain\n"
        + 'Requires-Dist: langchain-core>=0.3,<2; extra == "langchain"\n'
        + "Provides-Extra: llamaindex\n"
        + 'Requires-Dist: llama-index-core>=0.12,<1; extra == "llamaindex"\n'
        + "Description-Content-Type: text/markdown\n\n"
        + readme
    )


def build(version: str, tag: str, binary: Path, out: Path) -> Path:
    exe = f"{NAME}.exe" if tag.startswith("win") else NAME
    data = binary.read_bytes()
    dist_info = f"{NAME}-{version}.dist-info"
    data_dir = f"{NAME}-{version}.data"
    files: list[tuple[str, bytes, int]] = [
        (f"{data_dir}/scripts/{exe}", data, 0o755),
        (f"{dist_info}/METADATA", metadata(version).encode(), 0o644),
        (
            f"{dist_info}/WHEEL",
            f"Wheel-Version: 1.0\nGenerator: anymd build-wheels\nRoot-Is-Purelib: false\nTag: py3-none-{tag.split('.')[0]}\n"
            + "".join(f"Tag: py3-none-{t}\n" for t in tag.split(".")[1:]),
            0o644,
        ),
        (f"{dist_info}/licenses/LICENSE", (ROOT / "LICENSE").read_bytes(), 0o644),
    ]
    # The same platform wheel carries both the CLI and its thin Python API.
    for source in sorted((PYPI / NAME).rglob("*.py")):
        files.append((source.relative_to(PYPI).as_posix(), source.read_bytes(), 0o644))
    files.append((f"{NAME}/py.typed", b"", 0o644))
    files = [(n, d if isinstance(d, bytes) else d.encode(), m) for n, d, m in files]
    record = "".join(f"{n},{digest(d)},{len(d)}\n" for n, d, _ in files) + f"{dist_info}/RECORD,,\n"
    files.append((f"{dist_info}/RECORD", record.encode(), 0o644))

    path = out / f"{NAME}-{version}-py3-none-{tag}.whl"
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as wheel:
        for name, content, mode in files:
            info = zipfile.ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | mode) << 16
            wheel.writestr(info, content)
    return path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--binary", action="append", required=True, metavar="TAG=PATH")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    for spec in args.binary:
        tag, _, path = spec.partition("=")
        if not tag or not path or not Path(path).is_file():
            print(f"bad --binary {spec!r}", file=sys.stderr)
            return 2
        print(build(args.version, tag, Path(path), args.out))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
