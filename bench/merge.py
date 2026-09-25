#!/usr/bin/env python3
"""Merge sharded results of one tool into a single results JSON.

  python bench/merge.py OUT.json shard1.json shard2.json ...
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    out, *parts = sys.argv[1:]
    order = [d["id"] for d in json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]]
    meta, rows = None, {}
    for part in parts:
        data = json.loads(Path(part).read_text("utf-8"))
        meta = meta or data["meta"]
        for row in data["results"]:
            rows[row["doc"]] = row
    meta["shard"] = f"{len(parts)} shards" if len(parts) > 1 else meta["shard"]
    results = [rows[d] for d in order if d in rows]
    Path(out).write_text(json.dumps({"meta": meta, "results": results}, indent=1, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
