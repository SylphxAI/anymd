#!/usr/bin/env python3
"""Download the AgentDocBench corpus into DIR and verify every file's SHA-256.

Small license-clean files are committed under bench/files/ and copied from
there; everything else is downloaded from the pinned URL in bench/corpus.json.

  python bench/fetch.py [DIR]   (default: .cache/bench-corpus)
"""

import gzip
import hashlib
import json
import shutil
import ssl
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
AGENT = "Mozilla/5.0 (X11; Linux x86_64) AgentDocBench/1 (+https://github.com/SylphxAI/anymd/tree/main/bench)"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def download(url, target):
    # Integrity comes from the pinned SHA-256, not TLS: a server with an incomplete
    # certificate chain (ws.dgbas.gov.tw) is retried without certificate checks.
    context = None
    for attempt in range(5):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": AGENT})
            with urllib.request.urlopen(request, timeout=120, context=context) as response:
                body = response.read()
                if response.headers.get("Content-Encoding") == "gzip":
                    body = gzip.decompress(body)  # the Wayback Machine gzips some snapshots
            target.write_bytes(body)
            return
        except urllib.error.URLError as exc:
            if isinstance(exc.reason, ssl.SSLCertVerificationError) and context is None:
                context = ssl._create_unverified_context()  # noqa: S323 - hash-verified below
                continue
            if attempt == 4:
                raise
            print(f"  retry {attempt + 1} for {url}: {exc}", file=sys.stderr)
            time.sleep(5 * (attempt + 1))
        except Exception as exc:  # noqa: BLE001 - retry transient network errors
            if attempt == 4:
                raise
            print(f"  retry {attempt + 1} for {url}: {exc}", file=sys.stderr)
            time.sleep(5 * (attempt + 1))


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".cache/bench-corpus")
    out.mkdir(parents=True, exist_ok=True)
    failed = []
    for doc in json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]:
        target = out / doc["file"]
        if target.exists() and sha256(target) == doc["sha256"]:
            continue
        committed = HERE / "files" / doc["file"]
        tmp = target.with_suffix(target.suffix + ".part")
        try:
            if committed.exists():
                shutil.copyfile(committed, tmp)
            else:
                download(doc["url"], tmp)
        except Exception as exc:  # noqa: BLE001
            failed.append(f"{doc['id']}: {exc}")
            continue
        if sha256(tmp) != doc["sha256"]:
            failed.append(f"{doc['id']}: checksum mismatch for {doc['url']}")
            tmp.unlink()
            continue
        tmp.replace(target)
        print(f"fetched {doc['id']}")
    if failed:
        sys.exit("\n".join(["fetch failed:", *failed]))
    print(f"corpus ready in {out}")


if __name__ == "__main__":
    main()
