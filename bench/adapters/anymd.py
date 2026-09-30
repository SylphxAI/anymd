"""anymd (https://github.com/SylphxAI/anymd): the native CLI, one process per file.

Set ANYMD_BIN to the binary (default: `anymd` on PATH)."""

import os
import re
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


# The default read exports embedded images and marks each in place with a
# `![caption](path)` line followed by an `<!-- image: ... -->` comment. The
# ground truth has no image lines, so those pairs are stripped before scoring;
# the text and its reading order around the placeholders are what is measured.
# Other image lines (alt-text-only forms) are left as they were.
IMAGE_LINE = re.compile(r"^!\[.*\]\((<[^>]*>|[^)]*)\)[ \t]*$")
IMAGE_COMMENT = re.compile(r"^<!-- image: [^>]*-->[ \t]*$")


def read_output(stdout, out_dir):
    lines = stdout.decode("utf-8", "replace").splitlines(keepends=True)
    kept, stripped, i = [], False, 0
    while i < len(lines):
        if (
            i + 1 < len(lines)
            and IMAGE_LINE.match(lines[i].rstrip("\r\n"))
            and IMAGE_COMMENT.match(lines[i + 1].rstrip("\r\n"))
        ):
            stripped, i = True, i + 2
            continue
        kept.append(lines[i])
        i += 1
    text = "".join(kept)
    return re.sub(r"\n{3,}", "\n\n", text) if stripped else text
