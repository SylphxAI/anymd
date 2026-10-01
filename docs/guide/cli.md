# CLI

The same binary is a command-line converter, like MarkItDown but much faster. Install it with `npm install -g @sylphx/anymd`, or run it once with `npx -y @sylphx/anymd <file>`. From Python, `uvx anymd <file>` or `pip install anymd`; with Docker, `docker run --rm -v "$PWD:/data" ghcr.io/sylphxai/anymd <file>`.

```bash
anymd report.pdf > report.md                 # a file
anymd deck.pptx notes.docx budget.xlsx        # several files, each with a header
anymd https://example.com/article            # a web page (main content only)
cat scan.png | anymd - --ocr                 # stdin, with OCR
anymd paper.pdf --pages 1-3 --max-tokens 4000
anymd search "indemnification" contracts/ --glob '*.pdf'
anymd doctor                                 # lists the optional tools anymd found
```

## Document navigation

```bash
anymd outline report.pdf                     # indented tree
anymd outline report.pdf --format json       # preorder nodes with byte ranges
anymd report.pdf --node n1.2                  # section and its children
anymd report.pdf --node n1.2 --cursor 4:1200  # continue within that section
```

Outline and node reads default to no OCR or embedded image export, with Word
revision markup. `--ocr`, `--images` and `--revisions` select other options; pass
the same options to outline and node read.

See [outline](./tools#outline) for range and stable-id semantics. Search hits carry
node ids and title paths. With no `--node`, conversion and paging stay unchanged.

## Commands

| Command | What it does |
|---|---|
| `anymd <file\|url\|dir>... [options]` | Convert to Markdown on stdout |
| `anymd - [options]` | Convert stdin (the format is detected from the bytes) |
| `anymd outline <file\|url> [--format tree\|json]` | Show the document tree (tree by default; `--json` also selects JSON) |
| `anymd search <query> [path\|url...]` | Search files and directories (default: `.`) |
| `anymd mcp [--allow-dir=<path>]...` | Run the MCP server on stdio |
| `anymd setup [--dry-run] [--remove]` | Add anymd to the MCP clients on this machine; `--remove` undoes it |
| `anymd doctor` | Print the version and which optional tools were found |
| `anymd version` | Print the version |

With no file arguments and a piped stdin (which is how MCP clients launch it), `anymd` serves MCP over stdio, so `npx -y @sylphx/anymd` works as both a CLI and a server.

## Read options

| Option | Description |
|---|---|
| `--node <id>` | Read the section from `outline`; repeat it with `--cursor` to continue |
| `-p, --pages <spec>` | Pages, slides, sheets, or chapters, e.g. `1-5,8` |
| `-o, --output <file>` | Write to a file instead of stdout |
| `--max-tokens <n>` | Stop at a token budget and print a cursor. The CLI has no budget unless you set one. |
| `--cursor <cursor>` | Continue from a cursor |
| `--ocr` / `--no-ocr` | Force or disable OCR (default: automatic when tesseract is installed) |
| `--revisions <markup\|accept\|reject>` | Word tracked changes and comments: `markup` (default) writes them as CriticMarkup; `accept` or `reject` gives the text with every change accepted or rejected, without comments (see [Word](formats.md#word)) |
| `--images <refs\|none>` | `refs` (default) saves images embedded in PDF, DOCX, PPTX and EPUB files to the anymd cache and marks them in the Markdown; `none` leaves them out (see [Embedded images](formats.md#embedded-images)) |
| `--transcript` | Transcribe audio/video with bundled Qwen3-ASR |
| `--download-asr-model` | Download pinned Qwen3-ASR weights on first use (presence switch, rejects `=true`/`=false`; implies `--transcript`; see [Transcripts](formats.md#transcripts)) |
| `--front-matter` | Print the source/title/pages header (always on for several inputs) |

## Search options

| Option | Description |
|---|---|
| `--mode <m>` | `auto` (default), `literal`, or `ranked` (BM25) |
| `--glob <glob>` | Only files matching the glob, e.g. `'*.pdf'` |
| `--max <n>` | Maximum hits (default 20) |
| `-i, --case-sensitive` | Match case |
| `-w, --whole-word` | Match whole words |

## Other flags

| Option | Description |
|---|---|
| `--allow-dir=<path>` | Confine file access to this directory (repeatable). See [Security](./security). |
| `-h, --help` | Show help |
| `-V, --version` | Show the version |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | An input could not be read, or the output could not be written |
| `2` | Usage error (unknown option, missing value, no input) |

## Star reminder

After the fifth successful interactive run, anymd prints one line to stderr asking for a GitHub star, then never again. The run counter is the `star-hint` file in anymd's cache directory (`$ANYMD_CACHE_DIR`, else the platform cache directory). It stays silent in MCP server mode, when stderr is not a terminal, and when `CI` is set. Set `ANYMD_NO_STAR_HINT=1` to turn it off.

## Examples

Convert a folder of reports into Markdown files:

```bash
for f in reports/*.pdf; do anymd "$f" -o "${f%.pdf}.md"; done
```

Read a long PDF in chunks:

```bash
anymd book.pdf --max-tokens 8000            # ends with: Continue with cursor: "42"
anymd book.pdf --max-tokens 8000 --cursor 42
```

Ranked search when you do not know the exact wording:

```bash
anymd search "how is attention scaled" papers/ --mode ranked --max 5
```
