"""Kreuzberg (https://github.com/kreuzberg-dev/kreuzberg): extract_file_sync with Markdown output.

Kreuzberg's default config has OCR off; the adapter turns on its bundled tesseract
(English) so scanned pages are read, as the other tools do by default. Pages with a
text layer are not OCR'd."""

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
    from kreuzberg import ExtractionConfig, OcrConfig, extract_file_sync

    ocr = OcrConfig(backend="tesseract", language="eng")
    config = ExtractionConfig(output_format="markdown", use_cache=False, ocr=ocr)
    sys.stdout.write(extract_file_sync(sys.argv[1], config=config).content)
