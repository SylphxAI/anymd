#!/usr/bin/env python3
"""Download the pinned files of one tool or model from pins.json into a directory.

  fetch_asset.py tools|models NAME DEST
  fetch_asset.py source NAME        (prints REPO, TAG, COMMIT for `eval`)
Each file is verified against its SHA-256; entries marked `extract` are unpacked into DEST.
"""

import hashlib
import json
import subprocess
import sys
import tarfile
from pathlib import Path


def main() -> int:
    kind, name = sys.argv[1], sys.argv[2]
    pins = json.loads((Path(__file__).parent / "pins.json").read_text())
    if kind == "source":
        src = pins["sources"][name]
        print(f"REPO={src['repo']} TAG={src.get('tag', '')} COMMIT={src['commit']}")
        return 0
    dest = Path(sys.argv[3])
    dest.mkdir(parents=True, exist_ok=True)
    for entry in pins[kind][name]["files"]:
        target = dest / entry["name"]
        target.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["curl", "-fL", "--retry", "6", "--retry-delay", "10", "--retry-all-errors", "-o", str(target), entry["url"]],
            check=True,
        )
        digest = hashlib.sha256()
        with target.open("rb") as fh:
            for block in iter(lambda: fh.read(1 << 20), b""):
                digest.update(block)
        if digest.hexdigest() != entry["sha256"]:
            print(f"SHA-256 mismatch for {entry['name']}: {digest.hexdigest()}", file=sys.stderr)
            return 1
        if entry.get("extract"):
            with tarfile.open(target) as tar:
                tar.extractall(dest, filter="data")
            target.unlink()
    print(f"{kind}/{name}: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
