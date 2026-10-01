#!/usr/bin/env python3
"""OmniDocBench v1.6 harness for anymd: fetch pinned pages, run anymd on them, write predictions.

  harness.py predict --shard 1/4 --out pred [--limit N]   download this shard's pages, run anymd, write <page>.md
  harness.py gt --out gt.json [--limit N]                 write the ground truth (all pages, or the first N)

Pages are ordered by file name; --limit N keeps the first N pages of that order, then --shard splits them
round-robin. The dataset is pinned by Hugging Face commit; every image is verified against images.sha256
and the annotation file against ANNOTATION_SHA256. Nothing from the dataset is committed to this repository.
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REVISION = "d386947f7fc3bafdcd756c8485845a2f43a19875"  # opendatalab/OmniDocBench "add v1.6"
BASE = f"https://huggingface.co/datasets/opendatalab/OmniDocBench/resolve/{REVISION}"
ANNOTATION_SHA256 = "a45cd84b04ad8b793e775089640e6b681209abea33ead54c1828ddca35fae496"
OCR_HEADING = "## Text (OCR)"


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url, path):
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(5):
        try:
            with urllib.request.urlopen(url, timeout=120) as response, open(path, "wb") as out:
                while chunk := response.read(1 << 20):
                    out.write(chunk)
            return
        except OSError:
            if attempt == 4:
                raise
            time.sleep(2**attempt)


def manifest():
    entries = {}
    for line in (HERE / "images.sha256").read_text().splitlines():
        digest, name = line.split("  ", 1)
        entries[name] = digest
    return entries


def annotations(cache):
    path = cache / "OmniDocBench.json"
    if not path.exists():
        download(f"{BASE}/OmniDocBench.json", path)
    if sha256(path) != ANNOTATION_SHA256:
        sys.exit("OmniDocBench.json does not match its pinned SHA-256")
    return path, json.loads(path.read_text())


def page_names(pages, limit):
    names = sorted(p["page_info"]["image_path"] for p in pages)
    if limit < 0 or len(names) != len(set(names)):
        raise ValueError("negative page limit or duplicate ground-truth page IDs")
    outputs = [Path(n).with_suffix(".md").name for n in names]
    if len(outputs) != len(set(outputs)):
        raise ValueError("page IDs collide as prediction filenames")
    return names[:limit] if limit else names


def strip_metadata(markdown):
    """anymd prints an image as a metadata table plus a "## Text (OCR)" section; only the OCR text is page content."""
    head, sep, body = markdown.partition(OCR_HEADING)
    if not sep:
        return ""
    body = body.strip()
    return "" if body.startswith("_") and body.endswith("_") and "\n" not in body else body


def convert(binary, image, timeout):
    started = time.time()
    try:
        run = subprocess.run([binary, "--ocr", str(image)], capture_output=True, text=True, timeout=timeout)
        ok, text = run.returncode == 0, run.stdout
        error = None if ok else (run.stderr[-600:] or f"exit {run.returncode}")
        if ok and OCR_HEADING not in text:
            ok, error = False, "conversion output has no OCR section"
    except (subprocess.TimeoutExpired, OSError) as exc:
        ok, text, error = False, "", str(exc)
    return ok, strip_metadata(text) if ok else "", time.time() - started, error


def predict(args):
    cache = Path(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    (cache / "images").mkdir(exist_ok=True)
    _, pages = annotations(cache)
    names = page_names(pages, args.limit)
    index, total = (int(part) for part in args.shard.split("/"))
    if not names or not 1 <= index <= total:
        raise ValueError("benchmark needs pages and a valid i/n shard")
    mine = names[index - 1 :: total]
    digests = manifest()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    binary = os.environ.get("ANYMD_BIN", "anymd")

    def one(name):
        image = cache / "images" / name
        if not image.exists() or sha256(image) != digests[f"images/{name}"]:
            download(f"{BASE}/images/{urllib.parse.quote(name)}", image)
            if sha256(image) != digests[f"images/{name}"]:
                sys.exit(f"{name} does not match its pinned SHA-256")
        ok, markdown, seconds, error = convert(binary, image, args.timeout)
        prediction = out / Path(name).with_suffix(".md").name
        prediction.write_text(markdown, encoding="utf-8")
        return name, {"ok": ok, "seconds": round(seconds, 3), "chars": len(markdown),
                      "error": error, "sha256": sha256(prediction)}

    with concurrent.futures.ThreadPoolExecutor(args.workers) as pool:
        results = dict(pool.map(one, mine))
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()
    timings = {"anymd": version, "shard": args.shard, "pages": results,
               "planned_pages": names, "expected_pages": mine, "revision": REVISION}
    (out / f"timings-{index}of{total}.json").write_text(json.dumps(timings, indent=1))
    failed = sum(not r["ok"] for r in results.values())
    print(f"shard {args.shard}: {len(results)} pages, {failed} failed, {sum(r['seconds'] for r in results.values()):.0f} s of anymd time")
    return 1 if failed else 0


def validate(args):
    """Check the frozen ground truth, every shard outcome and prediction before evaluation."""
    names = page_names(json.loads(Path(args.gt).read_text()), 0)
    if not names:
        raise ValueError("no ground-truth pages")
    parts = Path(args.parts)
    predictions = {}
    for path in parts.rglob("*.md"):
        if path.name in predictions:
            raise ValueError(f"duplicate prediction: {path.name}")
        predictions[path.name] = path
    expected_files = {Path(n).with_suffix(".md").name for n in names}
    if set(predictions) != expected_files:
        raise ValueError("missing or unexpected prediction files")
    shards, seen, total = set(), set(), None
    for path in sorted(parts.rglob("timings-*.json")):
        data = json.loads(path.read_text())
        index, count = (int(p) for p in data["shard"].split("/"))
        if not 1 <= index <= count or index in shards or (total is not None and total != count):
            raise ValueError("invalid or duplicate prediction shard")
        total = count
        shards.add(index)
        expected = names[index - 1::count]
        if data["revision"] != REVISION or data["planned_pages"] != names or data["expected_pages"] != expected:
            raise ValueError("prediction shard does not match frozen ground truth")
        if set(data["pages"]) != set(expected):
            raise ValueError("missing or unexpected timing rows")
        for name, row in data["pages"].items():
            if name in seen or row.get("ok") is not True:
                raise ValueError(f"duplicate or failed conversion: {name}")
            prediction = predictions[Path(name).with_suffix(".md").name]
            if row["sha256"] != sha256(prediction) or row["chars"] != len(prediction.read_text(encoding="utf-8")):
                raise ValueError(f"prediction does not match timing record: {name}")
            seen.add(name)
    if total is None or shards != set(range(1, total + 1)) or seen != set(names):
        raise ValueError("incomplete timing shard coverage")
    print(f"Validated {len(names)} predictions and {total} successful shards")
    return 0


def gt(args):
    path, pages = annotations(Path(args.cache))
    if args.limit:
        keep = set(page_names(pages, args.limit))
        pages = [p for p in pages if p["page_info"]["image_path"] in keep]
    Path(args.out).write_text(json.dumps(pages, ensure_ascii=False))
    print(f"ground truth: {len(pages)} pages")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache", default="omnidocbench-data")
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("predict")
    p.add_argument("--shard", default="1/1")
    p.add_argument("--out", required=True)
    p.add_argument("--limit", type=int, default=0)
    p.add_argument("--workers", type=int, default=4)
    p.add_argument("--timeout", type=int, default=180)
    p.set_defaults(run=predict)
    g = sub.add_parser("gt")
    g.add_argument("--out", required=True)
    g.add_argument("--limit", type=int, default=0)
    g.set_defaults(run=gt)
    v = sub.add_parser("validate")
    v.add_argument("--gt", required=True)
    v.add_argument("--parts", required=True)
    v.set_defaults(run=validate)
    args = parser.parse_args()
    return args.run(args)


if __name__ == "__main__":
    raise SystemExit(main())
