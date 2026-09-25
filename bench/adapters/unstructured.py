"""Unstructured (https://github.com/Unstructured-IO/unstructured): partition() with its default
`auto` strategy (no hi_res layout model), then elements_to_md()."""

import sys
from importlib.metadata import version as _version

NAME = "unstructured"
URL = "https://github.com/Unstructured-IO/unstructured"
FORMATS = None
RUNS = 1


def version():
    return _version("unstructured")


def command(src, out_dir):
    return [sys.executable, __file__, str(src)]


if __name__ == "__main__":
    sys.path.remove(sys.path[0])  # this file shadows the package it wraps
    from unstructured.partition.auto import partition
    from unstructured.staging.base import elements_to_md

    sys.stdout.write(elements_to_md(partition(filename=sys.argv[1])))
