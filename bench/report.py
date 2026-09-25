#!/usr/bin/env python3
"""Render bench results JSON as Markdown tables (per document and totals)."""
import json
import sys
from collections import defaultdict

data = json.load(open(sys.argv[1]))
rows = data["results"]
tools = []
for row in rows:
    if row["tool"] not in tools:
        tools.append(row["tool"])

print(f"Benchmark run {data['meta']['date']} on {data['meta']['cpus']} CPUs ({data['meta']['machine']}), median of {data['meta']['runs']} runs (docling: 1).\n")
print("| document | " + " | ".join(tools) + " |")
print("|---|" + "---|" * len(tools))
docs = []
for row in rows:
    if row["doc"] not in docs:
        docs.append(row["doc"])
by = {(r["doc"], r["tool"]): r for r in rows}
for doc in docs:
    cells = []
    for tool in tools:
        r = by.get((doc, tool))
        if not r:
            cells.append("n/a")
        elif "error" in r:
            cells.append("error")
        else:
            parts = [f"{r['seconds']:.2f}s", f"{r['tokens']:,} tok"]
            if r.get("text_total"):
                parts.append(f"text {r['text_ok']}/{r['text_total']}")
            if r.get("table_total"):
                parts.append(f"tables {r['table_rows']}/{r['table_total']}")
            cells.append(" · ".join(parts))
    print(f"| {doc} | " + " | ".join(cells) + " |")

print("\n| tool | total time (s) | total tokens | sentences intact | table rows recovered | reading order ok |")
print("|---|---|---|---|---|---|")
for tool in tools:
    t = [r for r in rows if r["tool"] == tool and "error" not in r]
    pdf_docs = {r["doc"] for r in rows if r["tool"] == "pdftotext"}
    t_pdf = [r for r in t if r["doc"] in pdf_docs] if tool != "pdftotext" else t
    secs = sum(r["seconds"] for r in t)
    toks = sum(r["tokens"] for r in t)
    text_ok = sum(r.get("text_ok", 0) for r in t_pdf)
    text_total = sum(r.get("text_total", 0) for r in t_pdf)
    tab_ok = sum(r.get("table_rows", 0) for r in t_pdf)
    tab_total = sum(r.get("table_total", 0) for r in t_pdf)
    order = [r["order_ok"] for r in t_pdf if r.get("order_ok") is not None]
    errors = len([r for r in rows if r["tool"] == tool and "error" in r])
    note = f" ({errors} errors)" if errors else ""
    print(f"| {tool}{note} | {secs:.2f} | {toks:,} | {text_ok}/{text_total} | {tab_ok}/{tab_total} | {sum(order)}/{len(order)} |")
