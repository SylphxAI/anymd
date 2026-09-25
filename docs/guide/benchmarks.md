# Benchmarks

anymd against MarkItDown, Docling, Kreuzberg, and pdftotext on the same documents and the same machine.

Benchmark run 2026-09-25 on 4 CPUs (x86_64), median of 3 runs (docling: 1).

| document | anymd | markitdown | kreuzberg | docling | pdftotext |
|---|---|---|---|---|---|
| attention | 0.13s · 9,889 tok · text 4/4 · tables 14/14 | 3.00s · 11,994 tok · text 0/4 · tables 9/14 | 0.47s · 9,602 tok · text 4/4 · tables 0/14 | 75.94s · 10,023 tok · text 4/4 · tables 14/14 | 0.12s · 10,164 tok · text 4/4 · tables 0/14 |
| bert | 0.09s · 16,627 tok · text 3/3 · tables 2/2 | 2.99s · 28,087 tok · text 0/3 · tables 0/2 | 0.43s · 15,695 tok · text 3/3 · tables 0/2 | 80.79s · 16,201 tok · text 2/3 · tables 2/2 | 0.08s · 16,528 tok · text 3/3 · tables 0/2 |
| h8 | 0.08s · 34,741 tok · text 1/1 · tables 3/3 | 5.35s · 33,491 tok · text 1/1 · tables 0/3 | 0.60s · 32,610 tok · text 1/1 · tables 0/3 | 657.95s · 46,669 tok · text 1/1 · tables 2/3 | 0.08s · 33,524 tok · text 1/1 · tables 0/3 |
| cjk | 0.02s · 2,515 tok · text 1/1 · tables 1/1 | 1.45s · 2,574 tok · text 1/1 · tables 0/1 | 0.34s · 2,502 tok · text 1/1 · tables 0/1 | 25.19s · 2,595 tok · text 1/1 · tables 1/1 | 0.02s · 2,524 tok · text 1/1 · tables 0/1 |
| plz | 0.02s · 1,689 tok · text 1/1 | 1.45s · 1,942 tok · text 1/1 | 0.15s · 1,738 tok · text 1/1 | 20.91s · 1,713 tok · text 1/1 | 0.02s · 1,835 tok · text 1/1 |
| w9 | 0.03s · 8,785 tok · text 1/1 | 2.48s · 9,022 tok · text 1/1 | 0.20s · 8,550 tok · text 1/1 | 36.55s · 8,533 tok · text 1/1 | 0.03s · 8,933 tok · text 1/1 |
| borderless | 0.00s · 623 tok · text 1/1 · tables 6/6 | 0.92s · 771 tok · text 1/1 · tables 6/6 | 0.10s · 532 tok · text 1/1 · tables 0/6 | 24.22s · 809 tok · text 1/1 · tables 6/6 | 0.01s · 582 tok · text 1/1 · tables 0/6 |
| docx | 0.00s · 994 tok | 0.95s · 1,045 tok | 0.09s · 1,034 tok | 8.32s · 1,032 tok | n/a |
| pptx | 0.00s · 582 tok | 0.92s · 630 tok | 0.09s · 527 tok | 8.31s · 563 tok | n/a |
| xlsx | 0.00s · 351 tok | 0.90s · 464 tok | 0.09s · 464 tok | 8.24s · 554 tok | n/a |
| epub | 0.00s · 83 tok | 0.90s · 126 tok | 0.08s · 59 tok | 8.20s · 87 tok | n/a |
| html | 0.02s · 21,733 tok | 1.17s · 54,577 tok | 0.15s · 51,730 tok | 9.20s · 37,622 tok | n/a |

| tool | total time (s) | total tokens | sentences intact | table rows recovered | reading order ok |
|---|---|---|---|---|---|
| anymd | 0.40 | 98,612 | 12/12 | 26/26 | 5/5 |
| markitdown | 22.47 | 144,723 | 5/12 | 15/26 | 3/5 |
| kreuzberg | 2.79 | 125,043 | 12/12 | 0/26 | 5/5 |
| docling | 963.80 | 126,401 | 11/12 | 25/26 | 3/5 |
| pdftotext | 0.36 | 74,090 | 12/12 | 0/26 | 5/5 |

The official `@modelcontextprotocol/server-pdf` is left out because it has no headless text path: it renders PDFs in an interactive viewer, and its `read_pdf_bytes` tool returns base64-encoded bytes.

## Method

- **Fresh process per run.** Every tool runs as a new process, so start-up time is included, as an agent would pay it.
- **Median of 3.** Each document is converted 3 times per tool; the table shows the median time.
- **Tokens** are counted with tiktoken's `o200k_base` encoding.
- **Sentences intact** counts reference sentences that come out verbatim after whitespace and Markdown normalization. Glued words or split columns fail the check.
- **Table rows** counts ground-truth rows that come out as one Markdown table row with the cells in order.
- **Reading order**: on multi-column documents, key passages must also appear in the right order.

The reference sentences and table rows are in `bench/truth.json`. Competitor versions are pinned in `bench/requirements.txt`.

## Corpus

From `bench/corpus.json`:

| Document | Kind |
|---|---|
| `attention.pdf` | PDF: paper, 15 pages, tables |
| `bert.pdf` | PDF: two-column paper, 16 pages |
| `h8.pdf` | PDF: statistical tables, 22 pages |
| `cjk.pdf` | PDF: Traditional Chinese + table |
| `plz.pdf` | PDF: designed guide |
| `w9.pdf` | PDF: IRS form |
| `SPARSE-2024-INV-1234_borderless_table.pdf` | PDF: borderless tables |
| `test.docx` | Word |
| `test.pptx` | PowerPoint |
| `test.xlsx` | Excel |
| `test.epub` | EPUB |
| `test_wikipedia.html` | HTML: Wikipedia article |

The PDFs are SHA-256 verified from `corpus/markdown-regression.json`. The Office and web samples are microsoft/markitdown's MIT-licensed test files at a pinned commit.

## Reproduce

```bash
# 1. Fetch the corpus
bash bench/fetch.sh .cache/bench-corpus

# 2. Build anymd and install the other tools
cargo build --release -p pdf-reader-mcp-server
python3 -m venv .venv && .venv/bin/pip install -r bench/requirements.txt

# 3. Run and print the table
.venv/bin/python bench/run.py --corpus .cache/bench-corpus --anymd target/release/anymd \
  --tools anymd,markitdown,kreuzberg,docling,pdftotext --runs 3 --out bench-results.json
.venv/bin/python bench/report.py bench-results.json
```

`pdftotext` comes from poppler (`apt install poppler-utils`). Add `--docs attention,bert` to run a subset, or `--save-outputs DIR` to keep every tool's Markdown for inspection.

The [Benchmark workflow](https://github.com/SylphxAI/anymd/actions/workflows/benchmark.yml) runs the same steps on GitHub-hosted runners, on demand and whenever the harness changes, and posts the table to the job summary.
