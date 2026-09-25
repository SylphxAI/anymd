#!/usr/bin/env python3
"""anymd benchmark: speed, output tokens, text/order accuracy, and table rows.

Every tool runs as a fresh process on the same machine (process start-up is
part of the measured time, as an agent would pay it). Tokens are o200k
(tiktoken). Accuracy checks come from bench/truth.json.

Usage:
  python bench/run.py --corpus DIR --anymd PATH [--tools anymd,markitdown,...]
                      [--runs 3] [--out bench/results.json]
"""

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import unicodedata
from pathlib import Path

HERE = Path(__file__).resolve().parent
TIMEOUT = 900


def tool_commands(args):
    py = sys.executable
    kz = HERE / "kreuzberg_md.py"
    return {
        "anymd": lambda f, out: [args.anymd, str(f)],
        "markitdown": lambda f, out: [shutil.which("markitdown") or "markitdown", str(f)],
        "kreuzberg": lambda f, out: [py, str(kz), str(f)],
        "docling": lambda f, out: [shutil.which("docling") or "docling", str(f), "--to", "md",
                                   "--output", out, "--image-export-mode", "placeholder"],
        "pdftotext": lambda f, out: [shutil.which("pdftotext") or "pdftotext", "-enc", "UTF-8", str(f), "-"],
    }


def run_tool(name, cmd, out_dir):
    start = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT)
    elapsed = time.perf_counter() - start
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.decode("utf-8", "replace")[-500:])
    if name == "docling":
        files = list(Path(out_dir).glob("*.md"))
        text = files[0].read_text("utf-8") if files else ""
        for f in files:
            f.unlink()
    else:
        text = proc.stdout.decode("utf-8", "replace")
    return elapsed, text


def normalize(text):
    text = unicodedata.normalize("NFKC", text)
    text = re.sub(r"<!--.*?-->", " ", text, flags=re.S)
    text = re.sub(r"[#*_`|>^\\]", "", text)
    text = text.replace("“", '"').replace("”", '"').replace("’", "'")
    return re.sub(r"\s+", " ", text).strip()


def squash(text):
    return re.sub(r"\s+", "", normalize(text)).lower()


def table_rows(markdown):
    rows = []
    for line in markdown.splitlines():
        line = line.strip()
        if line.startswith("|") and line.endswith("|") and not re.match(r"^\|[\s:|-]+\|$", line):
            cells = [squash(c) for c in re.split(r"(?<!\\)\|", line[1:-1])]
            rows.append([c for c in cells if c])
    return rows


def row_found(expected, rows):
    want = [squash(c) for c in expected]
    for cells in rows:
        for start, cell in enumerate(cells):
            if want[0] not in cell:
                continue
            index = start + 1
            ok = True
            for value in want[1:]:
                while index < len(cells) and cells[index] != value:
                    index += 1
                if index >= len(cells):
                    ok = False
                    break
                index += 1
            if ok:
                return True
    return False


def score(doc_truth, markdown):
    flat = normalize(markdown)
    checks = [needle for needle in doc_truth.get("text", []) if normalize(needle) in flat]
    order_ok = True
    position = 0
    for needle in doc_truth.get("order", []):
        found = flat.find(needle, position)
        if found < 0:
            order_ok = False
            break
        position = found + len(needle)
    rows = table_rows(markdown)
    tables = doc_truth.get("tables", [])
    found_rows = sum(1 for row in tables if row_found(row, rows))
    return {
        "text_checks": f"{len(checks)}/{len(doc_truth.get('text', []))}",
        "text_ok": len(checks),
        "text_total": len(doc_truth.get("text", [])),
        "order_ok": order_ok if doc_truth.get("order") else None,
        "table_rows": found_rows,
        "table_total": len(tables),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--anymd", default="anymd")
    parser.add_argument("--tools", default="anymd,markitdown,kreuzberg,pdftotext")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--docs", default="")
    parser.add_argument("--out", default=str(HERE / "results.json"))
    parser.add_argument("--save-outputs", default="")
    args = parser.parse_args()

    import tiktoken

    enc = tiktoken.get_encoding("o200k_base")
    truth = json.loads((HERE / "truth.json").read_text())["docs"]
    manifest = json.loads((HERE / "corpus.json").read_text())["docs"]
    wanted = set(filter(None, args.docs.split(",")))
    commands = tool_commands(args)
    tools = [t for t in args.tools.split(",") if t]
    results = []
    out_dir = tempfile.mkdtemp(prefix="anymd-bench-")
    for doc in manifest:
        if wanted and doc["id"] not in wanted:
            continue
        path = Path(args.corpus) / doc["file"]
        if not path.exists():
            print(f"skip {doc['id']}: {path} missing", file=sys.stderr)
            continue
        for tool in tools:
            if tool == "pdftotext" and not doc["file"].endswith(".pdf"):
                continue
            runs = 1 if tool == "docling" else args.runs
            times, text, error = [], "", None
            for _ in range(runs):
                try:
                    elapsed, text = run_tool(tool, commands[tool](path, out_dir), out_dir)
                    times.append(elapsed)
                except Exception as exc:  # noqa: BLE001 - record and continue
                    error = str(exc)[:300]
                    break
            row = {"doc": doc["id"], "kind": doc["kind"], "tool": tool}
            if error:
                row["error"] = error
            else:
                row["seconds"] = round(statistics.median(times), 3)
                row["tokens"] = len(enc.encode(text, disallowed_special=()))
                row["bytes"] = len(text.encode())
                if doc["id"] in truth:
                    row.update(score(truth[doc["id"]], text))
                if args.save_outputs:
                    Path(args.save_outputs).mkdir(parents=True, exist_ok=True)
                    (Path(args.save_outputs) / f"{doc['id']}.{tool}.md").write_text(text)
            results.append(row)
            print(json.dumps(row), flush=True)
    meta = {
        "date": time.strftime("%Y-%m-%d"),
        "machine": os.uname().machine,
        "cpus": os.cpu_count(),
        "python": sys.version.split()[0],
        "runs": args.runs,
    }
    Path(args.out).write_text(json.dumps({"meta": meta, "results": results}, indent=1) + "\n")


if __name__ == "__main__":
    main()
