#!/usr/bin/env python3
"""Reject changed vendored packages whose version is already on crates.io.

Cargo's workspace packaging can use a local fork instead of its registry copy.
Compare the actual package payloads without compiling; only a registry 404
allows a new fork version through. Network failures fail closed.
"""
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
FORKS = ("vendor/adobe-cmap-parser", "vendor/pdf-extract")


def payload(data):
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        files = {}
        for member in archive.getmembers():
            if not member.isfile():
                continue
            name = member.name.split("/", 1)[1]
            # Cargo generates these from the checkout and dependency resolution.
            if name in ("Cargo.lock", ".cargo_vcs_info.json", "Cargo.toml.orig"):
                continue
            content = archive.extractfile(member).read()
            files[name] = tomllib.loads(content.decode()) if name == "Cargo.toml" else content
        return files


def check():
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    failed = False
    with tempfile.TemporaryDirectory(prefix="anymd-forks-") as target:
        for directory in FORKS:
            manifest = tomllib.loads((ROOT / directory / "Cargo.toml").read_text())
            name, version = (manifest["package"][key] for key in ("name", "version"))
            pin = next(dep for dep in workspace["workspace"]["dependencies"].values()
                       if dep.get("package") == name)
            if pin["version"] != version:
                raise RuntimeError(f"{name}: workspace pin does not match {version}")
            request = urllib.request.Request(
                f"https://crates.io/api/v1/crates/{name}/{version}",
                headers={"User-Agent": "anymd-vendored-crate-check"},
            )
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    json.load(response)
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
                print(f"{name} {version}: not published yet")
                continue
            request = urllib.request.Request(
                f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
                headers={"User-Agent": "anymd-vendored-crate-check"},
            )
            with urllib.request.urlopen(request, timeout=60) as response:
                published = payload(response.read())
            subprocess.run([
                "cargo", "package", "--locked", "--allow-dirty", "--no-verify",
                "--target-dir", target, "-p", name,
            ], cwd=ROOT, check=True)
            local = payload((Path(target) / "package" / f"{name}-{version}.crate").read_bytes())
            changed = sorted(key for key in published.keys() | local.keys()
                             if published.get(key) != local.get(key))
            if changed:
                failed = True
                print(f"{name} {version}: differs from crates.io; bump the fork and workspace pin")
                print(json.dumps(changed))
            else:
                print(f"{name} {version}: matches crates.io")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(check())
