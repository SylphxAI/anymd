# CLI

The same binary is a command-line converter, like MarkItDown but much faster. Install it with `npm install -g @sylphx/anymd`, or run it once with `npx -y @sylphx/anymd <file>`.

```bash
anymd report.pdf > report.md                 # a file
anymd deck.pptx notes.docx budget.xlsx        # several files, each with a header
anymd https://example.com/article            # a web page (main content only)
cat scan.png | anymd - --ocr                 # stdin, with OCR
anymd paper.pdf --pages 1-3 --max-tokens 4000
anymd search "indemnification" contracts/ --glob '*.pdf'
anymd doctor                                 # lists the optional tools anymd found
```

## Commands

| Command | What it does |
|---|---|
| `anymd <file\|url\|dir>... [options]` | Convert to Markdown on stdout |
| `anymd - [options]` | Convert stdin (the format is detected from the bytes) |
| `anymd search <query> [path\|url...]` | Search files and directories (default: `.`) |
| `anymd mcp [--allow-dir=<path>]...` | Run the MCP server on stdio |
| `anymd setup [--dry-run] [--remove]` | Add anymd to the MCP clients on this machine; `--remove` undoes it |
| `anymd doctor` | Print the version and which optional tools were found |
| `anymd version` | Print the version |

With no file arguments and a piped stdin (which is how MCP clients launch it), `anymd` serves MCP over stdio, so `npx -y @sylphx/anymd` works as both a CLI and a server.

## Read options

| Option | Description |
|---|---|
| `-p, --pages <spec>` | Pages, slides, sheets, or chapters, e.g. `1-5,8` |
| `-o, --output <file>` | Write to a file instead of stdout |
| `--max-tokens <n>` | Stop at a token budget and print a cursor. The CLI has no budget unless you set one. |
| `--cursor <cursor>` | Continue from a cursor |
| `--ocr` / `--no-ocr` | Force or disable OCR (default: automatic when tesseract is installed) |
| `--transcript` | Transcribe audio/video with a local whisper.cpp |
| `--download-whisper-model` | Download the ggml whisper model on first use (implies `--transcript`; see [Transcripts](formats.md#transcripts)) |
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
