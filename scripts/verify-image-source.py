#!/usr/bin/env python3
"""Print the common source commit for the staged GHCR Linux binaries."""

import argparse
import hashlib
import json
import re
from pathlib import Path

REPO = "SylphxAI/anymd"


def image_source(artifacts, version):
    """Bind staged Linux bytes to their original identities, not packaging HEAD."""
    sources = []
    for platform in ("linux-x64-gnu", "linux-arm64-gnu"):
        directory = artifacts / f"native-{platform}"
        identity = json.loads((directory / "identity.json").read_text())
        source = identity.get("source", {})
        if (identity.get("name") != "anymd" or identity.get("version") != version
                or identity.get("platform") != platform
                or source.get("repository") != REPO
                or not re.fullmatch(r"[0-9a-f]{40}", source.get("commit", ""))
                or identity.get("sha256") != hashlib.sha256((directory / "anymd").read_bytes()).hexdigest()):
            raise ValueError("image native identity mismatch")
        sources.append(source["commit"])
    if len(set(sources)) != 1:
        raise ValueError("mixed image native sources")
    return sources[0]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?", args.version):
        parser.error("invalid release version")
    print(image_source(args.artifacts, args.version))
