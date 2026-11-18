#!/usr/bin/env python3
"""Render the AgentDocBench leaderboard from bench/results/*.json.

Rewrites the block between `<!-- leaderboard:start -->` and
`<!-- leaderboard:end -->` in docs/guide/benchmarks.md, the headline block
between `<!-- headline:start -->` and `<!-- headline:end -->` in README.md (with
its `<!-- proof:start -->` line against each comparison tool), and
the head-to-head block between `<!-- versus:start -->` and `<!-- versus:end -->`
in each docs/vs/<tool>.md comparison page. The headline figures that a comparison
page and llms.txt quote by hand, outside that block, are checked against the results.
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
VERSUS = ROOT / "docs" / "vs"
# Hand-written copy that quotes a comparison page's headline figures, besides the page itself.
VERSUS_COPY = [ROOT / "docs" / "public" / "llms.txt"]
DISPLAY_NAMES = {"markitdown": "MarkItDown", "docling": "Docling", "marker": "Marker", "unstructured": "Unstructured"}

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


def load(results_dir):
    corpus = json.loads((HERE / "corpus.json").read_text("utf-8"))["docs"]
    tools = []
    for path in sorted(Path(results_dir).glob("*.json")):
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

    if "anymd" in summary:
        losses = []
        for category in summary_categories(corpus):
            leader = max(order, key=lambda n: summary[n]["per_category"][category])
            ours, best = summary["anymd"]["per_category"][category], summary[leader]["per_category"][category]
            if leader != "anymd" and best > ours:
                losses.append(f"{CATEGORY_NAMES.get(category, category)} ({leader} {pct(best)} vs {pct(ours)})")
        if losses:
            add("**Where anymd loses:** " + "; ".join(losses) + ".\n")

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


def fast_bullet(corpus, summary):
    """The README's speed claim: anymd's total on every document it converted, against each comparison tool."""
    ids = [d["id"] for d in corpus if summary["anymd"]["rows"].get(d["id"], {}).get("status") == "ok"]
    mine = sum(summary["anymd"]["rows"][d]["seconds"] for d in ids)
    full, partial = [], []
    for name in ("docling", "markitdown", "marker"):
        if name not in summary:
            continue
        rows = summary[name]["rows"]
        shared = [d for d in ids if rows.get(d, {}).get("status") == "ok"]
        theirs = sum(rows[d]["seconds"] for d in shared)
        if len(shared) == len(ids):
            full.append(f"{name} {secs(theirs)} ({theirs / mine:,.0f}×)")
        else:
            same = sum(summary["anymd"]["rows"][d]["seconds"] for d in shared)
            partial.append(f"{name} converted {len(shared)} of them in {secs(theirs)}, against anymd's {secs(same)} "
                           f"on the same {len(shared)} ({theirs / same:,.0f}×)")
    text = (f"- **Fast.** Native Rust converts in parallel, page by page. On the {len(ids)} benchmark documents, "
            f"anymd takes **{secs(mine)}** in total; {', '.join(full)}")
    if partial:
        text += "; " + "; ".join(partial)
    return text + ".\n"


def label(name):
    return DISPLAY_NAMES.get(name, name)


def totals(s, docs=None):
    """Seconds and output tokens over the documents a tool converted, or over `docs` (ids) only."""
    ok = [r for d, r in s["rows"].items() if r.get("status") == "ok" and (docs is None or d in docs)]
    return sum(r["seconds"] for r in ok), sum(r["tokens"] for r in ok)


def both_converted(corpus, a, b):
    """Ids of the documents both tools converted, so time and tokens compare the same work."""
    return {d["id"] for d in corpus
            if all(s["rows"].get(d["id"], {}).get("status") == "ok" for s in (a, b))}


def fewer(x, y):
    """How much less (or more) x is than y, as "32% fewer" or "12% more"."""
    return f"{abs(1 - x / y):.0%} {'fewer' if x < y else 'more'}"


def proof(corpus, summary, others, ours="anymd"):
    """The README's head-to-head lines, one per comparison page, each linked to its page.

    Only a tool that converted every document gets a line, so the times and tokens compare the same work."""
    n = len(corpus)
    full = [o for o in others if summary[ours]["coverage"][0] == n and summary[o]["coverage"][0] == n]
    out = [f"**Head to head on the same {n} documents** (AgentDocBench score · time for all {n} · output tokens):\n"]
    for other in sorted(full, key=lambda o: -summary[o]["overall"]):
        a, b = summary[ours], summary[other]
        (ta, ka), (tb, kb) = totals(a), totals(b)
        out.append(f"- [vs {label(other)}](https://sylphxai.github.io/anymd/vs/{other}"
                   f"?utm_source=github&utm_medium=readme&utm_content=vs_{other}): "
                   f"**{pct(a['overall'])}** vs {pct(b['overall'])} · **{secs(ta)}** vs {secs(tb)} · "
                   f"**{fewer(ka, kb)} tokens**")
    return "\n".join(out) + "\n" if full else ""


def versus(corpus, summary, meta, other, ours="anymd"):
    """A comparison page's head-to-head block: anymd against one other tool on every document."""
    a, b = summary[ours], summary[other]

    def version(name):
        v = meta[name]["tool_version"]
        return v[len(name) + 1:] if v.startswith(name + " ") else v

    def bold(x, y):
        if pct(x) == pct(y):
            return pct(x), pct(y)
        return (f"**{pct(x)}**", pct(y)) if x > y else (pct(x), f"**{pct(y)}**")

    shared = both_converted(corpus, a, b)
    complete = len(shared) == len(corpus)
    scope = "all converted documents" if complete else f"the {len(shared)} documents both converted"

    out = [f"| AgentDocBench | {ours} {version(ours)} | {label(other)} {version(other)} |", "|---|---|---|"]
    for title, key in [("Overall", "overall"), ("Sentences kept verbatim", "sentences"), ("Text F1", "text_f1"),
                       ("Reading order", "order"), ("Table cells F1", "tables")]:
        x, y = bold(a[key], b[key])
        out.append(f"| {title} | {x} | {y} |")
        if key == "overall":
            out.append(f"| Documents converted | {a['coverage'][0]}/{a['coverage'][1]} | "
                       f"{b['coverage'][0]}/{b['coverage'][1]} |")
    (ta, ka), (tb, kb) = totals(a, shared), totals(b, shared)
    fast = (f"**{secs(ta)}**", secs(tb)) if ta < tb else (secs(ta), f"**{secs(tb)}**")
    lean = (f"**{ka:,}**", f"{kb:,}") if ka < kb else (f"{ka:,}", f"**{kb:,}**")
    out.append(f"| Time, {scope} | {fast[0]} | {fast[1]} |")
    out.append(f"| Output tokens, {scope} | {lean[0]} | {lean[1]} |")

    wins, losses, ties = [], [], 0
    for doc in corpus:
        x, y = pct(doc_score(a["rows"].get(doc["id"]))), pct(doc_score(b["rows"].get(doc["id"])))
        if float(x) > float(y):
            wins.append(doc)
        elif float(x) < float(y):
            losses.append((doc, y, x))
        else:
            ties += 1
    out.append("")
    out.append(f"Document by document, {ours} scores higher on {len(wins)}, {label(other)} on {len(losses)}, "
               f"and {ties} {'is' if ties == 1 else 'are'} tied.\n")
    if not complete:
        def overall_on(s):
            by_category = {}
            for doc in corpus:
                if doc["id"] in shared:
                    by_category.setdefault(doc["category"], []).append(doc_score(s["rows"].get(doc["id"])))
            return mean(mean(v) for v in by_category.values())
        out.append(f"A document a tool did not convert scores 0. On the {len(shared)} documents both converted, "
                   f"the overall is **{pct(overall_on(a))}** for {ours} and **{pct(overall_on(b))}** for "
                   f"{label(other)}, and time and tokens above are measured on those {len(shared)}.\n")

    out.append("### By category\n")
    out.append(f"| Category | Documents | {ours} | {label(other)} |")
    out.append("|---|---|---|---|")
    for category, docs in summary_categories(corpus).items():
        x, y = bold(a["per_category"][category], b["per_category"][category])
        out.append(f"| {CATEGORY_NAMES.get(category, category)} | {len(docs)} | {x} | {y} |")
    out.append("")

    better = [f"{CATEGORY_NAMES.get(c, c)} ({pct(b['per_category'][c])} vs {pct(a['per_category'][c])})"
              for c in summary_categories(corpus) if b["per_category"][c] > a["per_category"][c]]
    better += [f"[`{doc['id']}`]({doc['url']}) ({y} vs {x})" for doc, y, x in losses]
    if better:
        out.append(f"**Where {label(other)} scores higher:** " + "; ".join(better) + ".\n")

    zero = [c for c in summary_categories(corpus) if b["per_category"][c] == 0 and a["per_category"][c] > 0]
    if zero:
        rest = [c for c in summary_categories(corpus) if c not in zero]
        names = ", ".join(CATEGORY_NAMES.get(c, c) for c in zero)
        out.append(f"Leave out {names}, where {label(other)} scores 0, and the overall is "
                   f"**{pct(mean(a['per_category'][c] for c in rest))}** for {ours} and "
                   f"**{pct(mean(b['per_category'][c] for c in rest))}** for {label(other)}.\n")
    facts = [pct(a["overall"]), pct(b["overall"]), secs(ta).removesuffix(" s"), secs(tb).removesuffix(" s"),
             f"{ka:,}", f"{kb:,}", fewer(ka, kb), version(ours), version(other),
             meta[ours]["date"], meta[other]["date"]]
    return "\n".join(out).rstrip("\n") + "\n", facts


def versus_pages(corpus, summary, meta):
    """Every docs/vs/<tool>.md page whose tool has results."""
    if not VERSUS.is_dir() or "anymd" not in summary:
        return {}
    return {path: versus(corpus, summary, meta, path.stem) for path in sorted(VERSUS.glob("*.md"))
            if path.stem in summary}


def splice(text, name, block):
    pattern = re.compile(rf"(<!-- {name}:start -->\n).*?(<!-- {name}:end -->)", re.S)
    if not pattern.search(text):
        raise SystemExit(f"missing <!-- {name}:start/end --> markers")
    inline = name == "fast"  # a list item: no blank lines around it
    return pattern.sub(lambda m: m.group(1) + ("" if inline else "\n") + block + ("" if inline else "\n") + m.group(2), text)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--print", action="store_true")
    parser.add_argument("--results", default=str(HERE / "results"), help="directory of results JSON")
    args = parser.parse_args()
    corpus, tools = load(args.results)
    board, summary, order = render(corpus, tools)
    if args.print:
        sys.stdout.write(board)
        return
    meta = {t["meta"]["tool"]: t["meta"] for t in tools if not t["meta"].get("skipped")}
    pages = versus_pages(corpus, summary, meta)
    targets = {
        DOCS: [("leaderboard", board), ("corpus", corpus_table(corpus))],
        README: [("headline", headline(summary, order)), ("fast", fast_bullet(corpus, summary)),
                 ("proof", proof(corpus, summary, [path.stem for path in pages]))],
        **{path: [("versus", block)] for path, (block, _) in pages.items()},
    }
    missing = []
    for path, (_, facts) in pages.items():
        for copy in [path, *VERSUS_COPY]:
            text = re.sub(r"<!-- versus:start -->.*?<!-- versus:end -->", "", copy.read_text("utf-8"), flags=re.S)
            absent = [f for f in facts if f not in text]
            if absent:
                missing.append(f"{copy.relative_to(ROOT)} lacks {', '.join(absent)} for {path.stem}")
    if missing:
        raise SystemExit("comparison copy does not match the results: " + "; ".join(missing))
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
