#!/usr/bin/env python3
"""Sign the companion manifest for a release.

Usage: sign-companion-manifest.py <version> <SHA256SUMS file> <signature output>

Signs the bytes "anymd-ocr-vlm <version>\n" + <manifest contents> with the
Ed25519 key in the ANYMD_COMPANION_SIGNING_KEY environment variable (base64url
raw 32-byte seed) and writes the base64url signature. The key is only read from
the environment and is never printed or written. anymd verifies the signature
against COMPANION_PUBLIC_KEYS in crates/anymd/src/ocr_vlm.rs.
"""
import base64
import os
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def b64u_decode(text: str) -> bytes:
    text = text.strip()
    return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    version, manifest_path, out_path = sys.argv[1:]
    seed = os.environ.get("ANYMD_COMPANION_SIGNING_KEY", "")
    if not seed:
        print("ANYMD_COMPANION_SIGNING_KEY is not set", file=sys.stderr)
        return 1
    try:
        key = Ed25519PrivateKey.from_private_bytes(b64u_decode(seed))
    except Exception:
        print("ANYMD_COMPANION_SIGNING_KEY is not a base64url Ed25519 seed", file=sys.stderr)
        return 1
    with open(manifest_path, "rb") as f:
        manifest = f.read()
    signature = key.sign(f"anymd-ocr-vlm {version}\n".encode() + manifest)
    with open(out_path, "w") as f:
        f.write(base64.urlsafe_b64encode(signature).rstrip(b"=").decode() + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
