#!/usr/bin/env python3
"""Write or verify the public artifact lock for the pinned Keel storefront."""

import hashlib
import json
import os
from pathlib import Path
import sys


def tree_hash(root: Path, names: tuple[str, ...] | None = None) -> str:
    digest = hashlib.sha256()
    if names is None:
        files = sorted((path for path in root.rglob("*") if path.is_file()), key=lambda path: path.as_posix())
    else:
        files = [root / name for name in names]
    for path in files:
        name = path.name if names is not None else path.relative_to(root).as_posix()
        data = path.read_bytes()
        digest.update(name.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(len(data)).encode("ascii"))
        digest.update(b"\0")
        digest.update(data)
        digest.update(b"\0")
    return digest.hexdigest()


def current_state(root: Path, revision: str) -> dict[str, object]:
    source = root / "docs" / "storefront"
    return {
        "schema_version": 1,
        "keel_revision": revision,
        "source_sha256": tree_hash(source, ("docs.toml", "pricing.md", "recover.md")),
        "output_sha256": tree_hash(source / "prebuilt"),
    }


def main() -> None:
    if len(sys.argv) != 4 or sys.argv[1] not in {"write", "verify"}:
        raise SystemExit("usage: storefront_artifact.py write|verify REPOSITORY_ROOT KEEL_REVISION")
    mode, root_argument, revision = sys.argv[1:]
    root = Path(root_argument).resolve()
    manifest = root / "docs" / "storefront" / "prebuilt.json"
    try:
        actual = current_state(root, revision)
        if mode == "write":
            temporary = manifest.with_suffix(".json.tmp")
            temporary.write_text(json.dumps(actual, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            os.replace(temporary, manifest)
            return
        expected = json.loads(manifest.read_text(encoding="utf-8"))
        if not isinstance(expected, dict):
            raise ValueError("prebuilt.json must contain an object")
        if expected != actual:
            changed = ",".join(sorted(key for key in actual if expected.get(key) != actual[key])) or "manifest"
            print(
                f"::error::Committed Keel storefront is stale or incomplete ({changed}). "
                "Run `bash scripts/build-storefront.sh` locally with Keel source access, "
                "then commit docs/storefront/prebuilt and prebuilt.json.",
                file=sys.stderr,
            )
            raise SystemExit(1)
    except (OSError, ValueError, TypeError) as error:
        print(f"::error::Keel storefront artifact verification failed: {error}", file=sys.stderr)
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
