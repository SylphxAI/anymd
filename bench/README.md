# AgentDocBench

AgentDocBench is an open benchmark for tools that turn documents into Markdown for AI agents. It checks
whether an agent reading the output gets the words verbatim, in reading order, with tables intact, and it
records how long that takes and how many tokens it costs.

Everything here is reproducible: the corpus is pinned by SHA-256, the ground truth is human-checkable JSON,
every tool runs through a small adapter, and the [Benchmark workflow](../.github/workflows/benchmark.yml)
reruns it all on GitHub-hosted runners. The leaderboard in
[docs/guide/benchmarks.md](../docs/guide/benchmarks.md) is generated from the committed results in
[`results/`](results/); no number is edited by hand.

The benchmark lives in the anymd repository, and anymd is one of the tools it measures. The scoring does
not know which tool produced an output, and the places where anymd loses are in the leaderboard like
everything else.

## Layout

| Path | What it is |
|---|---|
| `corpus.json` | The documents: id, category, format, pinned URL, SHA-256, license, attribution |
| `files/` | Small license-clean documents committed as-is (the rest are downloaded) |
| `truth/<id>.json` | Ground truth for each document (schema below) |
| `reference/<id>.txt` | Reference text for text F1 (see `reference.py`) |
| `adapters/<tool>.py` | One adapter per tool |
| `requirements/<tool>.txt` | Pinned tool versions |
| `fetch.py` | Downloads the corpus and verifies every SHA-256 |
| `run.py` | Converts the corpus with one tool and scores every output |
| `score.py` | The scoring; also rescores saved outputs |
| `merge.py` | Merges sharded results of one tool |
| `leaderboard.py` | Renders the leaderboard into the docs and README |
| `results/<tool>.json` | The committed results, one file per tool |

## Corpus

Every document is public domain (US federal government works, pre-1930 books), CC0, CC BY, CC BY-SA,
MIT-licensed test files, or published under an open government license (Japan PDL 1.0, Taiwan OGDL). Each
entry in `corpus.json` records the license and where it is stated. arXiv papers are included only when their
arXiv license is CC BY; papers under arXiv's default non-exclusive license are not.

Documents are downloaded from pinned URLs (versioned arXiv PDFs, dated government releases, Wayback Machine
`id_` snapshots for pages that change) and verified by SHA-256. Files of 200 KB or less whose license allows it
are committed in `files/`, so the benchmark survives link rot for those.

Categories: math-heavy papers, two-column papers, designed reports, financial and statistical tables,
fillable forms, scanned (image-only) PDFs, CJK documents, slides (PDF and PPTX), spreadsheets (XLSX and CSV),
Word, EPUB, and HTML.

## Ground truth

`truth/<id>.json`:

```json
{
  "id": "fed-h8",
  "derived_from": "How each field was obtained and which pages were checked by eye",
  "sentences": ["Complete sentences copied verbatim from the body text."],
  "order": ["Short headings or passage openings", "in the true reading order"],
  "tables": [
    {"caption": "Table 2 ...", "page": 3, "complete": true,
     "rows": [["Account", "2025 Aug", "..."], ["Bank credit", "18,665.6", "..."]]}
  ]
}
```

- **sentences**: 4 to 8 sentences from the start, middle, and end of the body text. For scanned documents they
  are read from the page images and are what a correct OCR must produce.
- **order**: 5 to 12 strings that each occur once in the document (never also in a table of contents or a
  running header). On two-column pages they include the bottom of one column and the top of the next.
- **tables**: `complete: true` tables list every row, header included, one string per column, `""` for an
  empty cell. Tables with multi-level spanning headers list their body rows and are marked `complete: false`.

Ground truth comes from the source itself (the PDF text layer, the DOCX/PPTX/XLSX XML, the EPUB/HTML markup),
and `derived_from` names the pages that were checked against the rendered page. It is plain JSON on purpose:
anyone can open a document next to its truth file and check it.

## Scoring

Each output is compared with its truth after the same normalization: Unicode NFKC, HTML comments and tags
removed, Markdown links and images reduced to their text, Markdown markup (`# * _ ` | > ~`) and escapes
removed, curly quotes made straight, soft hyphens dropped, whitespace collapsed, and whitespace between two
CJK characters removed.

| Score | Method |
|---|---|
| **Sentences** | Share of truth sentences found verbatim in the normalized output. A glued word, a split column, or a dropped line fails the sentence. |
| **Text F1** | Bag-of-words F1 between the output and `reference/<id>.txt`. Words are lowercased runs of letters and digits; each CJK character is one word. Precision drops when a tool adds noise (repeated headers, link URLs, image names); recall drops when it loses text. |
| **Reading order** | The longest chain of `order` strings whose positions in the output strictly increase, divided by the number of strings. Every occurrence of a string may be used, so a missing string costs one step and a swapped pair costs one. |
| **Table cells F1** | Each truth row is matched to the output table row with the longest in-order run of equal cells (cells compared without whitespace or case; empty cells ignored). Recall is matched cells over truth cells. For `complete` tables, precision is matched cells over all non-empty cells of the output tables the rows were found in, so merged or bloated tables lose precision. Tables are read from Markdown pipe tables and HTML `<table>` markup. Plain text scores 0: an agent cannot tell which number belongs to which column. |
| **Document score** | The mean of the scores that apply to the document. |
| **Category score** | The mean document score in the category. A failed, timed-out, or unsupported document scores 0. |
| **Overall** | The mean of the category scores, so each category weighs the same whatever its size. |
| **Time** | Wall time of a fresh process per conversion (start-up included, as an agent pays it), median of 3 runs; tools that take minutes per document get 1 run. Every tool first gets one untimed warm-up conversion, so model downloads are not timed. |
| **Output tokens** | `o200k_base` tokens (tiktoken) of the output. Fewer is better only when the scores are equal. |

Reference texts (`reference.py`): born-digital PDFs use the poppler `pdftotext` text layer, DOCX and PPTX
their XML text runs (slides with speaker notes), EPUB the text of every spine document. Scanned PDFs,
spreadsheets, CSV, and HTML have no reference text; they are scored on sentences, order, and tables. Because
`pdftotext` produced the PDF references, its own PDF text F1 is 100 by construction; read it as the ceiling
for that column.

Known limits: sentence checks are exact, so a tool that rewrites typography (ligatures aside) loses them;
equations are not scored beyond the words around them; images and figures are not scored.

## Running it

```bash
python3 bench/fetch.py .cache/bench-corpus                  # download + verify the corpus
python3 -m venv .venv && .venv/bin/pip install -r bench/requirements/harness.txt -r bench/requirements/markitdown.txt
cargo build --release -p pdf-reader-mcp-server             # anymd, into target/release/anymd

ANYMD_BIN=target/release/anymd .venv/bin/python bench/run.py --tool anymd \
  --corpus .cache/bench-corpus --out bench/results/anymd.json --save-outputs out/anymd
.venv/bin/python bench/leaderboard.py --print
```

`--docs id1,id2` runs a subset, `--shard 2/4` every fourth document from the second, and `score.py --outputs
DIR --results FILE` rescores saved outputs after a truth fix. Heavy tools (docling, marker, unstructured) are
meant for the workflow's runners, not a laptop.

The [Benchmark workflow](../.github/workflows/benchmark.yml) runs on every pull request that touches `bench/`
and on demand (`gh workflow run benchmark.yml -f tools=anymd,docling`). Each tool runs on its own 4-CPU
GitHub-hosted runner (docling and marker in 4 shards, unstructured in 2) with tesseract (English, Chinese,
Japanese), poppler, and pandoc installed for every tool alike. The Leaderboard job's summary shows the table,
and its `agentdocbench-results` artifact holds the merged results JSON and every Markdown output.

## Submitting a tool

1. Add `bench/adapters/<tool>.py`:

   ```python
   NAME = "mytool"
   URL = "https://github.com/me/mytool"
   FORMATS = None          # or {"pdf", "docx"}: the rest are recorded as unsupported (score 0)
   RUNS = 3                # timed runs per document; 1 for tools that take minutes

   def version(): ...      # the exact version string
   def command(src, out_dir): return ["mytool", str(src)]   # argv; Markdown on stdout
   # optional: def read_output(stdout, out_dir) -> str       # when the tool writes a file instead
   ```

   A Python library can wrap itself: `command` returns `[sys.executable, __file__, str(src)]` and the
   module's `__main__` block prints the Markdown (see `adapters/kreuzberg.py`).
2. Pin the tool in `bench/requirements/<tool>.txt` and add a matrix entry to the workflow.
3. Open a pull request. The Benchmark workflow runs every tool; download `agentdocbench-results`, copy
   `bench/results/<tool>.json` into the branch, run `python bench/leaderboard.py`, and commit both.

Use the tool's default settings, or say in the adapter's docstring which options you chose and why. No
network calls to hosted APIs: every tool must run locally on CPU.

Corpus or truth fixes are welcome the same way: change `corpus.json` or `truth/<id>.json`, say in the pull
request what was wrong, and regenerate the results.

## Licenses

The harness is MIT like the rest of the repository. Each document keeps its own license (see
`corpus.json`); `reference/` and `truth/` excerpts inherit the license of their document, and those taken
from CC BY-SA documents (Wikipedia) are CC BY-SA.
