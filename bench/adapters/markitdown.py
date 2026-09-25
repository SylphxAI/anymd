"""Microsoft MarkItDown (https://github.com/microsoft/markitdown), its CLI with default options."""

import shutil
from importlib.metadata import version as _version

NAME = "markitdown"
URL = "https://github.com/microsoft/markitdown"
FORMATS = None
RUNS = 3


def version():
    return _version("markitdown")


def command(src, out_dir):
    return [shutil.which("markitdown") or "markitdown", str(src)]
