"""Kreuzberg (https://github.com/kreuzberg-dev/kreuzberg): extract_file_sync with Markdown output."""

import sys
from importlib.metadata import version as _version

NAME = "kreuzberg"
URL = "https://github.com/kreuzberg-dev/kreuzberg"
FORMATS = None
RUNS = 3


def version():
    return _version("kreuzberg")


def command(src, out_dir):
    return [sys.executable, __file__, str(src)]


if __name__ == "__main__":
    sys.path.remove(sys.path[0])  # this file shadows the package it wraps
    from kreuzberg import ExtractionConfig, extract_file_sync

    config = ExtractionConfig(output_format="markdown", use_cache=False)
    sys.stdout.write(extract_file_sync(sys.argv[1], config=config).content)
