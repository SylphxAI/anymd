#!/usr/bin/env python3
"""Complete PyPI delivery using this run's natives or the exact release trust pack.

No compiler, global binary, latest-release fallback, or separate publisher.
Recovery uses the existing wheel owner to package the exact tagged API and
licence source. Releases without that API need a new version, not a retrofit.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile

REPO = "SylphxAI/anymd"
# Keep the existing wheel ABI/platform matrix and native artifact layout.
PLATFORMS = (
    ("linux-x64-gnu", "manylinux_2_17_x86_64.manylinux2014_x86_64", "anymd"),
    ("linux-arm64-gnu", "manylinux_2_17_aarch64.manylinux2014_aarch64", "anymd"),
    ("darwin-x64", "macosx_10_12_x86_64", "anymd"),
    ("darwin-arm64", "macosx_11_0_arm64", "anymd"),
    ("win32-x64-msvc", "win_amd64", "anymd.exe"),
)


def wheel_names(version):
    return {f"anymd-{version}-py3-none-{tag}.whl" for _, tag, _ in PLATFORMS}


def pypi_complete(version):
    """Only a version-specific 404 means absent; other probe errors fail."""
    try:
        with urllib.request.urlopen(f"https://pypi.org/pypi/anymd/{version}/json", timeout=30) as response:
            data = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return False
        raise
    if data["info"]["version"] != version:
        raise ValueError("PyPI returned a different version")
    files = data["urls"]
    expected = wheel_names(version)
    present = set()
    for entry in files:
        name = entry["filename"]
        if name not in expected:
            raise ValueError(f"unexpected PyPI wheel: {name}")
        if not re.fullmatch(r"[0-9a-f]{64}", entry["digests"]["sha256"]):
            raise ValueError(f"missing PyPI digest: {name}")
        if not entry.get("yanked", False):
            present.add(name)
    return present == expected


def run(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def extract_binary(archive, exe, destination):
    """Read only the named regular binary; never extract archive paths."""
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as bundle:
            members = [item for item in bundle.infolist() if item.filename == exe]
            if (len(members) != 1 or members[0].is_dir()
                    or stat.S_IFMT(members[0].external_attr >> 16) not in (0, stat.S_IFREG)):
                raise ValueError(f"missing or duplicate {exe}: {archive.name}")
            content = bundle.read(members[0])
    else:
        with tarfile.open(archive, "r:gz") as bundle:
            members = [item for item in bundle.getmembers() if item.name in (exe, f"./{exe}")]
            if len(members) != 1 or not members[0].isfile():
                raise ValueError(f"missing or duplicate {exe}: {archive.name}")
            content = bundle.extractfile(members[0]).read()
    if not content:
        raise ValueError(f"empty {exe}: {archive.name}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(content)
    destination.chmod(0o755)


def verify_digest(path, checksums, asset):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    matches = [line.split()[0] for line in checksums.splitlines()
               if len(line.split()) == 2 and line.split()[1] == path.name]
    if matches != [digest] or asset.get("digest") != f"sha256:{digest}":
        raise ValueError(f"release digest mismatch: {path.name}")


def recover(root, version, artifacts, work):
    tag = f"v{version}"
    release = json.loads(run("gh", "api", f"repos/{REPO}/releases/tags/{tag}"))
    if release["tag_name"] != tag or release.get("draft"):
        raise ValueError("wrong or draft GitHub release")
    # Resolve the actual tag, not target_commitish (which may be a moving branch).
    run("git", "fetch", "--no-tags", "origin", f"refs/tags/{tag}:refs/tags/{tag}", cwd=root)
    sha = run("git", "rev-parse", f"{tag}^{{commit}}", cwd=root)
    source = work / "source"
    source.mkdir(parents=True)
    source_files = ("packages/pypi", "LICENSE", "packages/anymd/package.json", "Cargo.toml")
    with (work / "source.tar").open("wb") as archive:
        subprocess.run(["git", "archive", sha, *source_files], cwd=root, stdout=archive, check=True)
    with tarfile.open(work / "source.tar") as archive:
        archive.extractall(source, filter="data")
    if json.loads((source / "packages/anymd/package.json").read_text())["version"] != version:
        raise ValueError("tagged source version mismatch")
    if not re.search(r'^version\s*=\s*"' + re.escape(version) + r'"\s*$',
                     (source / "Cargo.toml").read_text(), re.MULTILINE):
        raise ValueError("tagged Cargo version mismatch")
    if not (source / "packages/pypi/anymd/__init__.py").is_file():
        raise ValueError(f"{tag} has no tagged Python API; a new release containing the API is required")
    print(f"recovering {tag}: binary, API and licence source {sha}")
    assets = {asset["name"]: asset for asset in release["assets"]}
    names = [f"anymd-{platform}.{'zip' if exe.endswith('.exe') else 'tar.gz'}"
             for platform, _, exe in PLATFORMS]
    for name in ["SHA256SUMS", *names]:
        if name not in assets:
            raise ValueError(f"missing release target: {name}")
        run("gh", "release", "download", tag, "--repo", REPO, "--dir", str(work), "--pattern", name)
        # The trust job signs binaries before writing SHA256SUMS; the latter
        # is its completion marker, not an attested asset.
        if name != "SHA256SUMS":
            run("gh", "attestation", "verify", str(work / name), "--repo", REPO,
                "--signer-workflow", f"{REPO}/.github/workflows/release.yml", "--source-digest", sha)
    checksums = (work / "SHA256SUMS").read_text()
    sums_digest = hashlib.sha256((work / "SHA256SUMS").read_bytes()).hexdigest()
    if assets["SHA256SUMS"].get("digest") != f"sha256:{sums_digest}":
        raise ValueError("SHA256SUMS digest mismatch")
    for (platform, _, exe), name in zip(PLATFORMS, names):
        archive = work / name
        verify_digest(archive, checksums, assets[name])
        extract_binary(archive, exe, artifacts / f"native-{platform}" / exe)
    # Only the host target can execute here; all five are bound to the source
    # SHA through trust provenance and both published digest owners above.
    if run(str(artifacts / "native-linux-x64-gnu/anymd"), "version") != f"anymd {version}":
        raise ValueError("recovered binary version mismatch")
    return source


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


def deliver(root, artifacts, out):
    version = json.loads((root / "packages/anymd/package.json").read_text())["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?", version):
        raise ValueError("invalid release version")
    binaries = [artifacts / f"native-{platform}" / exe for platform, _, exe in PLATFORMS]
    present = [binary.is_file() and binary.stat().st_size > 0 for binary in binaries]
    if any(binary.exists() for binary in binaries) and not all(present):
        raise ValueError("incomplete native artifact matrix")
    if not any(present) and pypi_complete(version):
        return False
    with tempfile.TemporaryDirectory(prefix="anymd-wheels-") as temporary:
        if all(present):
            # Artifact downloads restore files as 0644, not their executable mode.
            for binary in binaries:
                binary.chmod(0o755)
            if run(str(binaries[0]), "version") != f"anymd {version}":
                raise ValueError("native binary version mismatch")
            source = root
        else:
            source = recover(root, version, artifacts, Path(temporary))
        # Reuse the current packaging implementation, but never its main-tree
        # payload during recovery. All Python files, README and licence come
        # from the same tagged source SHA as the verified binaries.
        spec = importlib.util.spec_from_file_location("build_wheels", root / "scripts/build-wheels.py")
        builder = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(builder)
        builder.ROOT = source
        builder.PYPI = source / "packages/pypi"
        out.mkdir(parents=True, exist_ok=True)
        for (_, tag, _), binary in zip(PLATFORMS, binaries):
            print(builder.build(version, tag, binary, out))
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, default=Path("artifacts"))
    parser.add_argument("--out", type=Path, default=Path("dist"))
    parser.add_argument("--image-source", action="store_true", help="verify staged Linux identities and print their source")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.image_source:
        version = json.loads((root / "packages/anymd/package.json").read_text())["version"]
        print(image_source(args.artifacts.resolve(), version))
        return
    built = deliver(root, args.artifacts.resolve(), args.out.resolve())
    result = f"built={str(built).lower()}\n"
    print(result, end="")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(result)


if __name__ == "__main__":
    main()
