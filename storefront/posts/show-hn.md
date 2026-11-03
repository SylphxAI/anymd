# Show HN draft

Post time: booked window 2026-10-20 14:00–16:00Z (win#911). Paste the title
and the body; add the first comment within minutes (HN culture expects the
author's context comment).

Posting is held until the product owner reviews this draft. Every claim below
is rendered from the repository's own sources: the benchmark block from
`bench/leaderboard.py` (README, 38 documents), the price and Pro facts from
`docs/pro.md`, and the install line from `README.md`. Do not add first-person
experiences that did not happen (persona rule: no invented experiences).

## Title

Show HN: anymd – every file an agent reads becomes clean Markdown, on your machine

## Body (first comment)

Agents read PDFs, Office files, EPUB, HTML, images, audio and video badly. I
built anymd so one local tool turns any of them into clean Markdown the agent
can cite.

It is one Rust binary that is both the MCP server and the CLI, published to
npm as `@sylphx/anymd`. No API key, nothing uploaded.

What it does:

- **One call, every format.** PDF, Word, PowerPoint, Excel, EPUB, HTML and web
  pages, images (OCR), audio and video (metadata, subtitles, transcripts).
  It also takes a whole directory, and `search` looks across everything.
- **Reading order and tables that survive.** A layout engine rebuilds words
  from glyph gaps, puts two-column papers in reading order, and recovers
  tables including borderless ones. Text stays as printed: no glued words, no
  scrambled columns.
- **Citations.** Pages come back with `<!-- page 3 -->` anchors and a small
  front-matter header, so the agent can point at where it read something.
- **Local and private.** Nothing is uploaded. OCR uses local models or
  tesseract; transcripts use ffmpeg and a bundled ASR model. Both downloads are
  explicit opt-in.

On the 38-document AgentDocBench set, anymd converts all 38 in 11.5 s total.
docling takes 2,432.4 s (212×); markitdown 75.6 s (7×); marker converted 30 of
them in 7,104.5 s, against anymd's 4.64 s on the same 30 (1,532×). The bench
and its numbers are in the repo (`bench/`, `docs/guide/benchmarks.md`), and the
README block is regenerated from the committed results.

Install:

```bash
npx -y @sylphx/anymd setup            # add anymd to every MCP client on this machine
npx -y @sylphx/anymd report.pdf > report.md
```

Node 18+ is the only requirement; npm installs the native binary for your
platform. Source is MIT: https://github.com/SylphxAI/anymd

There is a paid add-on, anymd Pro (one-time offline licence), that adds
verifiable video frames (timestamps and SHA-256 per frame) and cite-check (it
proves a quote is on the page and at the spot it claims). The core — reading,
outlining, searching, inspecting, OCR, transcripts, the MCP server and the CLI
— stays free and MIT. Nothing that was free moved to Pro.

Happy to answer questions about the layout and table recovery, the MCP tool
schemas, or why local beats uploading your documents to a hosted converter.
