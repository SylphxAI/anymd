#!/usr/bin/env python3
"""AgentDocBench runner: convert every corpus document with one tool and score it.

Each conversion is a fresh process (start-up is part of the time, as an agent
pays it). Every tool first gets one untimed warm-up conversion, so model
downloads and cold disk caches are not timed. Tokens are o200k_base.

  python bench/run.py --tool anymd --corpus .cache/bench-corpus \
      --out bench/results/anymd.json --save-outputs outputs/anymd

Adapters live in bench/adapters/<tool>.py; see bench/README.md.
"""

import argparse
import importlib.util
import json
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import score as scoring  # noqa: E402

BENCHMARK_VERSION = "1"


def load_adapter(name):
    path = HERE / "adapters" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(f"adapter_{name}", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def convert(adapter, src, timeout):
    with tempfile.TemporaryDirectory(prefix="agentdocbench-") as out_dir:
        start = time.perf_counter()
        proc = subprocess.run(adapter.command(src, Path(out_dir)), capture_output=True, timeout=timeout)
        elapsed = time.perf_counter() - start
        if proc.returncode != 0:
            raise RuntimeError(proc.stderr.decode("utf-8", "replace")[-600:] or f"exit {proc.returncode}")
        if hasattr(adapter, "read_output"):
            text = adapter.read_output(proc.stdout, Path(out_dir))
        else:
            text = proc.stdout.decode("utf-8", "replace")
    return elapsed, text


def supports(adapter, doc):
    formats = getattr(adapter, "FORMATS", None)
    return formats is None or doc["format"] in formats


def select(docs, only, shard):
    if only:
        docs = [d for d in docs if d["id"] in only]
    if shard:
        index, count = (int(x) for x in shard.split("/"))
        docs = [d for i, d in enumerate(docs) if i % count == index - 1]
    return docs


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tool", required=True, help="adapter name (bench/adapters/<tool>.py)")
    parser.add_argument("--corpus", required=True, help="directory filled by bench/fetch.py")
    parser.add_argument("--out", required=True, help="results JSON to write")
    parser.add_argument("--runs", type=int, default=0, help="timed runs per document (default: adapter's RUNS)")
    parser.add_argument("--timeout", type=int, default=900, help="seconds per conversion")
    parser.add_argument("--docs", default="", help="comma-separated document ids (default: all)")
    parser.add_argument("--shard", default="", help="i/n: run every n-th document starting at i")
    parser.add_argument("--save-outputs", default="", help="directory for each document's Markdown")
    args = parser.parse_args()

    import tiktoken

    enc = tiktoken.get_encoding("o200k_base")
    adapter = load_adapter(args.tool)
    runs = args.runs or getattr(adapter, "RUNS", 3)
    manifest = json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]
    docs = select(manifest, set(filter(None, args.docs.split(","))), args.shard)
    corpus = Path(args.corpus)

    warm = next((d for d in docs if supports(adapter, d) and (corpus / d["file"]).exists()), None)
    if warm:
        try:
            convert(adapter, corpus / warm["file"], args.timeout)
        except Exception as exc:  # noqa: BLE001 - the timed run records the failure
            print(f"warm-up on {warm['id']} failed: {exc}", file=sys.stderr)

    results = []
    for doc in docs:
        row = {"doc": doc["id"], "category": doc["category"], "format": doc["format"]}
        path = corpus / doc["file"]
        if not supports(adapter, doc):
            row["status"] = "unsupported"
        elif not path.exists():
            row["status"] = "missing"
        else:
            times, text = [], ""
            try:
                for _ in range(runs):
                    elapsed, text = convert(adapter, path, args.timeout)
                    times.append(elapsed)
                row["status"] = "ok"
            except subprocess.TimeoutExpired:
                row["status"], row["error"] = "timeout", f"over {args.timeout}s"
            except Exception as exc:  # noqa: BLE001 - record and continue
                row["status"], row["error"] = "error", str(exc)[-400:]
            if row["status"] == "ok":
                row["seconds"] = round(statistics.median(times), 3)
                row["tokens"] = len(enc.encode(text, disallowed_special=()))
                row["bytes"] = len(text.encode())
                row["metrics"] = scoring.score(doc["id"], text)
                if args.save_outputs:
                    out = Path(args.save_outputs)
                    out.mkdir(parents=True, exist_ok=True)
                    (out / f"{doc['id']}.md").write_text(text, "utf-8")
        results.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)

    try:
        tool_version = adapter.version()
    except Exception as exc:  # noqa: BLE001
        tool_version = f"unknown ({exc})"
    meta = {
        "benchmark": "AgentDocBench",
        "benchmark_version": BENCHMARK_VERSION,
        "tool": getattr(adapter, "NAME", args.tool),
        "tool_version": tool_version,
        "tool_url": getattr(adapter, "URL", ""),
        "date": time.strftime("%Y-%m-%d"),
        "runs": runs,
        "timeout": args.timeout,
        "shard": args.shard or "1/1",
        "machine": f"{platform.system()} {platform.machine()}, {os.cpu_count()} CPUs",
        "runner": "github-hosted" if os.environ.get("GITHUB_ACTIONS") else "local",
        "run_url": (
            f"{os.environ['GITHUB_SERVER_URL']}/{os.environ['GITHUB_REPOSITORY']}/actions/runs/{os.environ['GITHUB_RUN_ID']}"
            if os.environ.get("GITHUB_RUN_ID")
            else ""
        ),
        "commit": os.environ.get("GITHUB_SHA", ""),
        "python": platform.python_version(),
    }
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps({"meta": meta, "results": results}, indent=1, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
