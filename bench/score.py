#!/usr/bin/env python3
"""AgentDocBench scoring: compare one Markdown output with one ground-truth file.

The method is documented in bench/README.md. In short, every document yields
up to four scores in [0, 1]:

  sentences  share of key sentences found verbatim (after normalization)
  text_f1    bag-of-words F1 against the reference text (bench/reference/)
  order      longest in-order chain of the reading-order strings / count
  tables     cell-level F1 (precision only on tables marked complete)

and the document score is their mean. Run it directly to rescore saved
outputs without converting again:

  python bench/score.py --outputs DIR --results bench/results/anymd.json
"""

import argparse
import html
import json
import re
import unicodedata
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path

HERE = Path(__file__).resolve().parent

CJK = "぀-ヿ㐀-䶿一-鿿豈-﫿가-힯　-〿＀-￯"
TOKEN = re.compile(rf"[{CJK}]|[^\W_{CJK}]+")
QUOTES = str.maketrans({"“": '"', "”": '"', "„": '"', "‟": '"', "‘": "'", "’": "'", "‚": "'", "‛": "'"})


def normalize(text):
    """Markdown/HTML-insensitive, whitespace-insensitive text for matching."""
    text = unicodedata.normalize("NFKC", text)
    text = re.sub(r"<!--(.*?)-->", r" \1 ", text, flags=re.S)  # comments are read by agents too
    target = r"\((?:[^()\s]|\([^()\s]*\))*(?:\s+\"[^\"]*\")?\)"  # (url) with one level of (parens)
    text = re.sub(r"!\[([^\]]*)\]" + target, r"\1", text)  # images -> alt text
    text = re.sub(r"\[([^\]]*)\]" + target, r"\1", text)  # links -> link text
    text = re.sub(r"</?[A-Za-z][^>]*>", " ", text)  # HTML tags
    text = html.unescape(text)
    text = re.sub(r"\\([\\`*_{}\[\]()#+\-.!|<>~])", r"\1", text)  # Markdown escapes
    text = re.sub(r"[#*_`|>~]", " ", text)
    text = text.translate(QUOTES).replace("­", "")
    return re.sub(r"\s+", " ", text).strip()


def squash(text):
    return re.sub(r"\s+", "", normalize(text)).lower()


def tokens(text):
    return TOKEN.findall(normalize(text).lower())


# --- sentences ---------------------------------------------------------------


HAS_CJK = re.compile(rf"[{CJK}]")


def contains(flat, dense, needle):
    """Needle in the normalized output. CJK text has no word spaces, and extractors
    scatter spaces around its digits and brackets, so CJK needles ignore whitespace."""
    needle = normalize(needle)
    if HAS_CJK.search(needle):
        return re.sub(r"\s", "", needle) in dense
    return needle in flat


def sentence_score(truth, flat, dense):
    wanted = truth.get("sentences", [])
    found = [s for s in wanted if contains(flat, dense, s)]
    return len(found), len(wanted)


# --- bag-of-words F1 ---------------------------------------------------------


def text_f1(reference, markdown):
    ref, out = Counter(tokens(reference)), Counter(tokens(markdown))
    overlap = sum((ref & out).values())
    if not overlap:
        return 0.0, 0.0, 0.0
    precision = overlap / sum(out.values())
    recall = overlap / sum(ref.values())
    return precision, recall, 2 * precision * recall / (precision + recall)


# --- reading order -----------------------------------------------------------


def occurrences(haystack, needle, limit=200):
    spots, start = [], 0
    while len(spots) < limit:
        found = haystack.find(needle, start)
        if found < 0:
            break
        spots.append(found)
        start = found + 1
    return spots


def order_score(truth, dense):
    """Longest chain of order strings whose positions strictly increase, / n.
    Positions are taken in the whitespace-free output: this measures order only."""
    needles = [re.sub(r"\s", "", normalize(s)) for s in truth.get("order", [])]
    if not needles:
        return None
    best = []  # best[i] = list of (position, chain length ending here)
    top = 0
    for i, needle in enumerate(needles):
        here = []
        for pos in occurrences(dense, needle):
            length = 1
            for j in range(i):
                for prev_pos, prev_len in best[j]:
                    if prev_pos < pos and prev_len + 1 > length:
                        length = prev_len + 1
            here.append((pos, length))
            top = max(top, length)
        best.append(here)
    return top / len(needles)


# --- tables ------------------------------------------------------------------


class _HtmlTables(HTMLParser):
    def __init__(self):
        super().__init__()
        self.tables, self._rows, self._cell, self._depth = [], None, None, 0

    def handle_starttag(self, tag, attrs):
        if tag == "table":
            self._depth += 1
            if self._depth == 1:
                self._rows = []
        elif tag == "tr" and self._rows is not None:
            self._rows.append([])
        elif tag in ("td", "th") and self._rows is not None:
            self._cell = []
        elif tag == "br" and self._cell is not None:
            self._cell.append(" ")

    def handle_endtag(self, tag):
        if tag in ("td", "th") and self._cell is not None and self._rows:
            self._rows[-1].append("".join(self._cell))
            self._cell = None
        elif tag == "table":
            self._depth -= 1
            if self._depth == 0 and self._rows is not None:
                self.tables.append([r for r in self._rows if r])
                self._rows = None

    def handle_data(self, data):
        if self._cell is not None:
            self._cell.append(data)


def parse_tables(markdown):
    """Pipe tables and HTML tables in the output, as lists of rows of squashed cells."""
    tables, current = [], []
    for line in markdown.splitlines():
        line = line.strip()
        if line.startswith("|") and line.count("|") >= 2:
            if re.fullmatch(r"\|[\s:|+-]*\|?", line):
                continue
            body = line[1:-1] if line.endswith("|") else line[1:]
            current.append(re.split(r"(?<!\\)\|", body))
        elif current:
            tables.append(current)
            current = []
    if current:
        tables.append(current)
    if "<table" in markdown.lower():
        parser = _HtmlTables()
        parser.feed(markdown)
        tables.extend(parser.tables)
    cleaned = []
    for table in tables:
        rows = [[squash(c) for c in row] for row in table]
        rows = [[c for c in row if c] for row in rows]
        cleaned.append([row for row in rows if row])
    return [t for t in cleaned if t]


MARKER = re.compile(r"[0-9a-z*\u2020\u2021\u00a7]{1,2}")
NUMBER = re.compile(r"-?[\d,]*\.?\d+")
LETTER_MARKER = re.compile(r"[a-z*\u2020\u2021\u00a7]{1,2}")
ROW_NUMBER = re.compile(r"\d{1,3}\.?")


def cell_eq(want, got):
    """Equal cells, ignoring printed footnote markers and row numbers: a text cell
    (one with a letter) may carry a row number before it and a marker of up to two
    characters after it; a number may carry a marker of up to two letters after it."""
    if want == got:
        return True
    if NUMBER.fullmatch(want) and NUMBER.fullmatch(got) and "." in got:
        # a spreadsheet shows 2.5402 for a stored 2.540163689: compare at the shown precision
        places = len(want.partition(".")[2])
        if len(got.partition(".")[2]) > places:
            return f"{float(got.replace(',', '')):,.{places}f}".replace(",", "") == want.replace(",", "")
    if not got.startswith(want) and want not in got:
        return False
    head, _, tail = got.partition(want)
    if re.search(r"[^\W\d_]", want):
        return (not head or bool(ROW_NUMBER.fullmatch(head))) and (not tail or bool(MARKER.fullmatch(tail)))
    return not head and bool(LETTER_MARKER.fullmatch(tail))


def lcs(a, b):
    """Length of the longest common subsequence of two cell lists (cell_eq cells)."""
    if not a or not b:
        return 0
    prev = [0] * (len(b) + 1)
    for x in a:
        cur = [0]
        for j, y in enumerate(b):
            cur.append(prev[j] + 1 if cell_eq(x, y) else max(prev[j + 1], cur[j]))
        prev = cur
    return prev[-1]


def table_score(truth, markdown):
    tables = truth.get("tables", [])
    if not tables:
        return None
    out = parse_tables(markdown)
    flat_rows = [(ti, row) for ti, table in enumerate(out) for row in table]
    rows_total = rows_found = cells_total = cells_hit = 0
    prec_hit = prec_den = 0
    for table in tables:
        used_tables, credit = set(), {}
        for row in table["rows"]:
            want = [squash(c) for c in row]
            want = [c for c in want if c]
            if not want:
                continue
            rows_total += 1
            cells_total += len(want)
            best, best_row = 0, None
            for index, (_, cells) in enumerate(flat_rows):
                if len(cells) < best:
                    continue
                hit = lcs(want, cells)
                if hit > best:
                    best, best_row = hit, index
                    if hit == len(want):
                        break
            cells_hit += best
            if best == len(want):
                rows_found += 1
            if best_row is not None and best * 2 >= len(want):
                used_tables.add(flat_rows[best_row][0])
                credit[best_row] = max(credit.get(best_row, 0), best)
        if table.get("complete"):
            # an output row matched by several truth rows is credited once
            prec_hit += sum(hit for index, hit in credit.items() if flat_rows[index][0] in used_tables)
            prec_den += sum(len(r) for ti in used_tables for r in out[ti])
    recall = cells_hit / cells_total if cells_total else 0.0
    precision = prec_hit / prec_den if prec_den else (None if not any(t.get("complete") for t in tables) else 0.0)
    if precision is None:
        f1 = recall
    else:
        f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return {
        "rows_found": rows_found,
        "rows_total": rows_total,
        "cells_hit": cells_hit,
        "cells_total": cells_total,
        "cell_recall": round(recall, 4),
        "cell_precision": None if precision is None else round(precision, 4),
        "cell_f1": round(f1, 4),
    }


# --- per document ------------------------------------------------------------


def load_truth(doc_id):
    path = HERE / "truth" / f"{doc_id}.json"
    return json.loads(path.read_text("utf-8")) if path.exists() else {}


def load_reference(doc_id):
    path = HERE / "reference" / f"{doc_id}.txt"
    return path.read_text("utf-8") if path.exists() else None


def score(doc_id, markdown, truth=None, reference=None):
    truth = load_truth(doc_id) if truth is None else truth
    reference = load_reference(doc_id) if reference is None else reference
    flat = normalize(markdown)
    dense = re.sub(r"\s", "", flat)
    metrics, parts = {}, []
    found, total = sentence_score(truth, flat, dense)
    if total:
        metrics["sentences"] = [found, total]
        parts.append(found / total)
    if reference:
        precision, recall, f1 = text_f1(reference, markdown)
        metrics["text"] = {"precision": round(precision, 4), "recall": round(recall, 4), "f1": round(f1, 4)}
        parts.append(f1)
    order = order_score(truth, dense)
    if order is not None:
        metrics["order"] = round(order, 4)
        parts.append(order)
    tables = table_score(truth, markdown)
    if tables is not None:
        metrics["tables"] = tables
        parts.append(tables["cell_f1"])
    metrics["score"] = round(sum(parts) / len(parts), 4) if parts else None
    return metrics


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--outputs", required=True, help="directory of <doc>.md files saved by run.py")
    parser.add_argument("--results", required=True, help="results JSON to rescore in place")
    args = parser.parse_args()
    data = json.loads(Path(args.results).read_text())
    for row in data["results"]:
        if row.get("status") != "ok":
            continue
        md = Path(args.outputs) / f"{row['doc']}.md"
        if md.exists():
            row["metrics"] = score(row["doc"], md.read_text("utf-8"))
    Path(args.results).write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
