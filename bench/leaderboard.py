#!/usr/bin/env python3
"""Render the AgentDocBench leaderboard from bench/results/*.json.

Rewrites the block between `<!-- leaderboard:start -->` and
`<!-- leaderboard:end -->` in docs/guide/benchmarks.md, and the headline block
between `<!-- headline:start -->` and `<!-- headline:end -->` in README.md.
Numbers are never edited by hand: change the results, then run

  python bench/leaderboard.py            # rewrite the docs
  python bench/leaderboard.py --check    # fail if the docs are stale
  python bench/leaderboard.py --print    # print the leaderboard only
"""

import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
DOCS = ROOT / "docs" / "guide" / "benchmarks.md"
README = ROOT / "README.md"

CATEGORY_NAMES = {
    "paper": "Papers (math)",
    "two-column": "Two-column papers",
    "report": "Reports",
    "financial": "Financial tables",
    "form": "Forms",
    "scanned": "Scanned (OCR)",
    "cjk": "CJK",
    "slides": "Slides",
    "spreadsheet": "Spreadsheets",
    "docx": "Word",
    "epub": "EPUB",
    "html": "HTML",
}


def load():
    corpus = json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]
    tools = []
    for path in sorted((HERE / "results").glob("*.json")):
        data = json.loads(path.read_text("utf-8"))
        tools.append(data)
    return corpus, tools


def doc_score(row):
    if not row or row.get("status") != "ok":
        return 0.0
    return row.get("metrics", {}).get("score") or 0.0


def pct(value):
    return "–" if value is None else f"{100 * value:.1f}"


def secs(value):
    return f"{value:,.1f} s" if value >= 10 else f"{value:.2f} s"


def mean(values):
    values = list(values)
    return sum(values) / len(values) if values else None


def summarize(corpus, data):
    rows = {r["doc"]: r for r in data["results"]}
    categories = {}
    for doc in corpus:
        categories.setdefault(doc["category"], []).append(doc_score(rows.get(doc["id"])))
    per_category = {c: mean(v) for c, v in categories.items()}
    ok = [r for r in data["results"] if r.get("status") == "ok"]

    def component(key, docs):
        values = []
        for doc in docs:
            row = rows.get(doc["id"])
            metrics = row.get("metrics", {}) if row and row.get("status") == "ok" else {}
            values.append(metrics.get(key))
        return values

    truth = {d["id"]: json.loads((HERE / "truth" / f"{d['id']}.json").read_text("utf-8")) for d in corpus}
    sent_found = sent_total = 0
    for doc in corpus:
        total = len(truth[doc["id"]].get("sentences", []))
        sent_total += total
        row = rows.get(doc["id"])
        if row and row.get("status") == "ok" and "sentences" in row.get("metrics", {}):
            sent_found += row["metrics"]["sentences"][0]
    ref_docs = [d for d in corpus if (HERE / "reference" / f"{d['id']}.txt").exists()]
    order_docs = [d for d in corpus if truth[d["id"]].get("order")]
    table_docs = [d for d in corpus if truth[d["id"]].get("tables")]
    text = [(v or {}).get("f1", 0.0) for v in component("text", ref_docs)]
    order = [v or 0.0 for v in component("order", order_docs)]
    tables = [(v or {}).get("cell_f1", 0.0) for v in component("tables", table_docs)]
    return {
        "overall": mean(per_category.values()),
        "per_category": per_category,
        "coverage": (len(ok), len(corpus)),
        "sentences": sent_found / sent_total if sent_total else None,
        "text_f1": mean(text),
        "order": mean(order),
        "tables": mean(tables),
        "rows": rows,
    }


def render(corpus, tools):
    ran = [t for t in tools if not t["meta"].get("skipped")]
    skipped = [t for t in tools if t["meta"].get("skipped")]
    summary = {t["meta"]["tool"]: summarize(corpus, t) for t in ran}
    order = sorted(summary, key=lambda name: -summary[name]["overall"])
    meta = {t["meta"]["tool"]: t["meta"] for t in ran}
    out = []
    add = out.append

    common = [d for d in corpus if all(summary[n]["rows"].get(d["id"], {}).get("status") == "ok" for n in order)]
    add(f"{len(corpus)} documents in {len(summary_categories(corpus))} categories. "
        "Scores are 0–100; higher is better. Every failed, timed-out, or unsupported document scores 0.\n")
    add("| Tool | Overall | Docs converted | Sentences | Text F1 | Reading order | Table cells F1 | Time, all docs | Output tokens |")
    add("|---|---|---|---|---|---|---|---|---|")
    top = max(summary[n]["overall"] for n in order) if order else 0
    for name in order:
        s = summary[name]
        ok = [r for r in s["rows"].values() if r.get("status") == "ok"]
        overall = f"**{pct(s['overall'])}**" if s["overall"] == top else pct(s["overall"])
        time_all = sum(r["seconds"] for r in ok)
        tokens_all = sum(r["tokens"] for r in ok)
        add(f"| {name} | {overall} | {s['coverage'][0]}/{s['coverage'][1]} | {pct(s['sentences'])} | "
            f"{pct(s['text_f1'])} | {pct(s['order'])} | {pct(s['tables'])} | {secs(time_all)} | {tokens_all:,} |")
    add("")
    add("Overall is the mean of the category scores below, so every category weighs the same. Time and tokens "
        "cover only the documents each tool converted; the table after the categories compares tools on the same documents.\n")

    add("### By category\n")
    add("| Category | Docs | " + " | ".join(order) + " |")
    add("|---|---|" + "---|" * len(order))
    for category, docs in summary_categories(corpus).items():
        cells = []
        best = max(summary[n]["per_category"][category] for n in order)
        for name in order:
            value = summary[name]["per_category"][category]
            text = pct(value)
            cells.append(f"**{text}**" if value == best and best > 0 else text)
        add(f"| {CATEGORY_NAMES.get(category, category)} | {len(docs)} | " + " | ".join(cells) + " |")
    add("")

    add(f"### Speed and tokens on the {len(common)} documents every tool converted\n")
    add("| Tool | Total time | Median per document | Output tokens |")
    add("|---|---|---|---|")
    for name in order:
        rows = [summary[name]["rows"][d["id"]] for d in common]
        seconds = sorted(r["seconds"] for r in rows)
        median = seconds[len(seconds) // 2] if seconds else 0
        add(f"| {name} | {secs(sum(seconds))} | {secs(median)} | {sum(r['tokens'] for r in rows):,} |")
    add("")

    add("<details><summary>Every document (score · time)</summary>\n")
    add("| Document | Category | " + " | ".join(order) + " |")
    add("|---|---|" + "---|" * len(order))
    for doc in corpus:
        cells = []
        for name in order:
            row = summary[name]["rows"].get(doc["id"])
            if not row:
                cells.append("–")
            elif row["status"] != "ok":
                cells.append(row["status"])
            else:
                cells.append(f"{pct(row['metrics'].get('score'))} · {secs(row['seconds'])}")
        add(f"| `{doc['id']}` | {doc['category']} | " + " | ".join(cells) + " |")
    add("\n</details>\n")

    add("### Tools\n")
    add("| Tool | Version | Timed runs | Run |")
    add("|---|---|---|---|")
    for name in order:
        m = meta[name]
        run = f"[{m['date']}]({m['run_url']})" if m.get("run_url") else m["date"]
        add(f"| [{name}]({m['tool_url']}) | `{m['tool_version']}` | {m['runs']} | {run} |")
    for t in skipped:
        m = t["meta"]
        add(f"| [{m['tool']}]({m.get('tool_url', '')}) | `{m.get('tool_version', '')}` | skipped | {m['skipped']} |")
    add("")
    machines = sorted({m["machine"] for m in meta.values()})
    add(f"Machine: {', '.join(machines)} ({'GitHub-hosted runner' if all(m['runner'] == 'github-hosted' for m in meta.values()) else 'mixed runners'}).")
    add("pdftotext is the reference extractor for born-digital PDFs (bench/reference/), so its text F1 on those "
        "documents is 100 by construction; it outputs plain text, so it scores 0 on tables and on non-PDF formats.")
    return "\n".join(out) + "\n", summary, order


def summary_categories(corpus):
    grouped = {}
    for key in CATEGORY_NAMES:
        docs = [d for d in corpus if d["category"] == key]
        if docs:
            grouped[key] = docs
    for doc in corpus:
        if doc["category"] not in CATEGORY_NAMES:
            grouped.setdefault(doc["category"], []).append(doc)
    return grouped


def corpus_table(corpus):
    out = ["| Document | Category | Format | Pages | License | Source |", "|---|---|---|---|---|---|"]
    for doc in corpus:
        out.append(f"| `{doc['id']}` | {doc['category']} | {doc['format']} | {doc.get('pages') or '–'} | "
                   f"{doc['license']} | [{doc['attribution']}]({doc['url']}) |")
    return "\n".join(out) + "\n"


def headline(summary, order):
    out = ["| | " + " | ".join(f"**{n}**" if n == "anymd" else n for n in order) + " |", "|---|" + "---|" * len(order)]
    out.append("| Overall score | " + " | ".join(pct(summary[n]["overall"]) for n in order) + " |")
    out.append("| Table cells F1 | " + " | ".join(pct(summary[n]["tables"]) for n in order) + " |")
    out.append("| Reading order | " + " | ".join(pct(summary[n]["order"]) for n in order) + " |")
    out.append("| Docs converted | " + " | ".join(f"{summary[n]['coverage'][0]}/{summary[n]['coverage'][1]}" for n in order) + " |")
    out.append("| Time, all docs | " + " | ".join(
        secs(sum(r["seconds"] for r in summary[n]["rows"].values() if r.get("status") == "ok")) for n in order) + " |")
    return "\n".join(out) + "\n"


def splice(text, name, block):
    pattern = re.compile(rf"(<!-- {name}:start -->\n).*?(<!-- {name}:end -->)", re.S)
    if not pattern.search(text):
        raise SystemExit(f"missing <!-- {name}:start/end --> markers")
    return pattern.sub(lambda m: m.group(1) + "\n" + block + "\n" + m.group(2), text)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--print", action="store_true")
    args = parser.parse_args()
    corpus, tools = load()
    board, summary, order = render(corpus, tools)
    if args.print:
        sys.stdout.write(board)
        return
    targets = {
        DOCS: [("leaderboard", board), ("corpus", corpus_table(corpus))],
        README: [("headline", headline(summary, order))],
    }
    stale = []
    for path, blocks in targets.items():
        text = path.read_text("utf-8")
        new = text
        for name, block in blocks:
            new = splice(new, name, block)
        if new != text:
            stale.append(str(path.relative_to(ROOT)))
            if not args.check:
                path.write_text(new, "utf-8")
    if args.check and stale:
        raise SystemExit("stale leaderboard in: " + ", ".join(stale) + " (run python bench/leaderboard.py)")


if __name__ == "__main__":
    main()
