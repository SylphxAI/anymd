"""IBM Docling (https://github.com/docling-project/docling): its CLI, Markdown output, default
pipeline (layout + table models, OCR on bitmap regions), CPU only, images as placeholders."""

import shutil
from importlib.metadata import version as _version
from pathlib import Path

NAME = "docling"
URL = "https://github.com/docling-project/docling"
FORMATS = None
RUNS = 1  # minutes per document on CPU; one timed run after the warm-up


def version():
    return _version("docling")


def command(src, out_dir):
    return [shutil.which("docling") or "docling", str(src), "--to", "md", "--output", str(out_dir),
            "--image-export-mode", "placeholder"]


def read_output(stdout, out_dir):
    files = sorted(Path(out_dir).glob("*.md"))
    return files[0].read_text("utf-8") if files else ""
