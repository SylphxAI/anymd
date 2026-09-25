"""anymd (https://github.com/SylphxAI/anymd): the native CLI, one process per file.

Set ANYMD_BIN to the binary (default: `anymd` on PATH)."""

import os
import subprocess

NAME = "anymd"
URL = "https://github.com/SylphxAI/anymd"
FORMATS = None  # every format in the corpus
RUNS = 3


def binary():
    return os.environ.get("ANYMD_BIN", "anymd")


def version():
    return subprocess.run([binary(), "--version"], capture_output=True, text=True).stdout.strip()


def command(src, out_dir):
    return [binary(), str(src)]
