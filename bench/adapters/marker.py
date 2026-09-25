"""Datalab Marker (https://github.com/datalab-to/marker): `marker_single`, Markdown output,
CPU (its default `fast` mode on CPU), no hosted LLM. Marker 2 runs its surya OCR model through a
local llama.cpp server (the workflow installs a pinned llama-server build); the model weights
download on the warm-up run."""

import shutil
from importlib.metadata import version as _version
from pathlib import Path

NAME = "marker"
URL = "https://github.com/datalab-to/marker"
FORMATS = {"pdf", "docx", "pptx", "xlsx", "epub", "html"}  # marker-pdf[full]; no CSV
RUNS = 1


def version():
    return _version("marker-pdf")


def command(src, out_dir):
    return [shutil.which("marker_single") or "marker_single", str(src), "--output_format", "markdown",
            "--output_dir", str(out_dir), "--disable_image_extraction"]


def read_output(stdout, out_dir):
    files = sorted(Path(out_dir).rglob("*.md"))
    return files[0].read_text("utf-8") if files else ""
