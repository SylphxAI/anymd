"""pdftotext from poppler-utils: plain text, PDFs only. A baseline, not a Markdown converter."""

import shutil
import subprocess

NAME = "pdftotext"
URL = "https://poppler.freedesktop.org/"
FORMATS = {"pdf"}
RUNS = 3


def version():
    proc = subprocess.run(["pdftotext", "-v"], capture_output=True, text=True)
    return (proc.stderr or proc.stdout).splitlines()[0].strip()


def command(src, out_dir):
    return [shutil.which("pdftotext") or "pdftotext", "-enc", "UTF-8", str(src), "-"]
