#!/usr/bin/env python3
"""Merge a complete, checked set of AgentDocBench shards.

  python bench/merge.py OUT.json shard1.json shard2.json ...
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def merge(parts, corpus_ids):
    if not parts:
        raise ValueError("no benchmark shards supplied")
    first = parts[0]["meta"]
    planned = first["planned_ids"]
    if not planned or len(planned) != len(set(planned)) or not set(planned) <= set(corpus_ids):
        raise ValueError("invalid planned document IDs")
    rows, shards = {}, set()
    total = None
    failed = False
    for data in parts:
        meta = data["meta"]
        if any(meta.get(key) != first.get(key) for key in ("planned_ids", "tool", "commit", "benchmark_version")):
            raise ValueError("shards belong to different benchmark plans or producers")
        index, count = (int(x) for x in meta["shard"].split("/"))
        if not 1 <= index <= count or (total is not None and total != count) or index in shards:
            raise ValueError("invalid or duplicate shard")
        total = count
        shards.add(index)
        expected = planned[index - 1::count]
        actual = [r["doc"] for r in data["results"]]
        if meta["expected_ids"] != expected or len(actual) != len(set(actual)) or set(actual) != set(expected):
            raise ValueError("missing, duplicate or unexpected result IDs")
        if meta.get("status") not in ("ok", "failed"):
            raise ValueError("missing benchmark outcome")
        failed |= meta["status"] != "ok"
        for row in data["results"]:
            if row["status"] not in ("ok", "unsupported", "missing", "timeout", "error"):
                raise ValueError("unknown conversion outcome")
            failed |= row["status"] not in ("ok", "unsupported", "timeout")
            rows[row["doc"]] = row
    if shards != set(range(1, total + 1)) or set(rows) != set(planned):
        raise ValueError("incomplete benchmark shard coverage")
    meta = dict(first, shard=f"{total} shards", expected_ids=planned, status="failed" if failed else "ok")
    return {"meta": meta, "results": [rows[d] for d in corpus_ids if d in rows]}


def main():
    out, *paths = sys.argv[1:]
    order = [d["id"] for d in json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]]
    data = merge([json.loads(Path(p).read_text("utf-8")) for p in paths], order)
    Path(out).write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n")
    return 1 if data["meta"]["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
