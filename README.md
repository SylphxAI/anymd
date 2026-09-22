<div align="center">

<img src="docs/public/logo.svg" alt="Citra" width="108" height="108" />

# Citra

### Give your AI agent eyes for PDFs — with proof.

**Local-first PDF evidence for agents.** One call returns structured text, tables, OCR and
**page-level citations your agent can defend** — not invent.

[![npm](https://img.shields.io/npm/v/@sylphx/citra?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://www.npmjs.com/package/@sylphx/citra)
[![downloads](https://img.shields.io/npm/dm/@sylphx/citra?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://www.npmjs.com/package/@sylphx/citra)
[![stars](https://img.shields.io/github/stars/SylphxAI/citra?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://github.com/SylphxAI/citra/stargazers)
[![license](https://img.shields.io/badge/license-MIT-c3f53c?style=flat-square&labelColor=0a0d07)](LICENSE)
[![MCP registry](https://img.shields.io/badge/MCP%20registry-io.github.SylphxAI%2Fcitra-c3f53c?style=flat-square&labelColor=0a0d07)](https://registry.modelcontextprotocol.io/servers/io.github.SylphxAI%2Fcitra)

**npm** [`@sylphx/citra`](https://www.npmjs.com/package/@sylphx/citra) · **bin** `citra` · **MCP** `io.github.SylphxAI/citra`

</div>

---

## The problem

Most PDF tools hand an agent a wall of text. The agent then *guesses* — the page
number, the table grid, the region behind the claim. And a confidently wrong
answer costs more than *"I can't tell."*

## The difference

<table>
<tr><td width="50%">A text dump says</td><td width="50%"><strong>Citra returns</strong></td></tr>
<tr>
<td valign="top">

```
Revenue was about $12M.
```

</td>
<td valign="top">

```
page 1 · table p1-table-1 · 3 cols / 9 cells
bbox 72,151 → 454,79 · confidence 0.92
continuation → page 2 (same column count)
```

</td>
</tr>
</table>

Locators in, citations out. A human can check the claim.

## One call. Locators included.

```json
{
  "status": "ok",
  "route":   { "engine": "rust-core", "path": "rust-read-pdf-v1" },
  "source":  { "hash": "99d313eb…", "path": "…/selectable-table-v1.pdf" },
  "results": [{
    "data": {
      "table_info": [{
        "page": 1,
        "bounding_box": { "left": 72, "top": 151, "right": 454.8, "bottom": 79 },
        "colCount": 3,
        "cellCount": 9,
        "confidence": 0.92,
        "provenance": { "engine": "pdf-reader-core", "source": "selectable_text" },
        "continuation": {
          "role": "starts",
          "groupId": "table-continuation-p1-table-1-p2-table-1",
          "signals": ["same_column_count", "repeated_header_candidate"]
        },
        "quality": {
          "completeness": 0.79,
          "cellBoundingBoxCoverage": 0.89,
          "signals": ["missing_cells", "merged_cell_candidates"]
        }
      }]
    }
  }],
  "gaps": []
}
```

<sub>Excerpt of a **real** `read_pdf` response against `test/fixtures/differential/v3014-selectable-table-v1.pdf`
(paths shortened). The table is detected **and** linked to its continuation on page 2 — and when
Citra cannot prove something, it says so in `gaps` instead of guessing.</sub>

## Install in 30 seconds

```bash
npx -y @sylphx/citra
```

No Docker. No API key. No global install. That starts a **stdio MCP server** your agent
can use immediately.

| Your client | Setup |
| --- | --- |
| **Any agent / CLI** | `npx -y @sylphx/citra` |
| **Claude Code** | `claude mcp add citra -- npx -y @sylphx/citra` |
| **Claude Desktop / Cursor / VS Code / Codex** | `"command": "npx", "args": ["-y", "@sylphx/citra"]` |
| **Global CLI** | `npm i -g @sylphx/citra` → `citra` |

<details>
<summary><strong>Claude Desktop / Cursor / VS Code — full <code>mcpServers</code> snippet</strong></summary>

```json
{
  "mcpServers": {
    "citra": {
      "command": "npx",
      "args": ["-y", "@sylphx/citra"]
    }
  }
}
```

</details>

## Why teams pick Citra

- **Zero-config.** A real `npx` MCP server — not a 20-step bootstrap.
- **Evidence, not vibes.** Page, geometry, table cells, provenance. Citations a human can check.
- **Local-first.** PDFs stay on the machine. No required cloud vision API, no document upload.
- **Fail closed.** No matching native binary? The process refuses to start. Never a silent engine fallback.
- **Native and small.** A Rust PDF engine behind a thin launcher — not PDF.js plus a large JS tree.

## What you get

Three tools. One surface. Few, powerful, obvious.

| Tool | What an agent uses it for |
| --- | --- |
| `read_pdf` | The smart default — markdown, tables with cells and geometry, structure, optional OCR, citation-ready chunks |
| `search_pdf` | Cheap literal retrieval first: page and bounding-box locators before a deep read |
| `pdf_evidence` | Focused verification: `inspect`, `render_page`, `extract_regions`, `ocr_pages`, `analyze_regions` |

Full option and result reference: **[docs/api](https://sylphxai.github.io/citra/api/)**

## Proof, method-bounded

| | |
| --- | --- |
| **≥ 10.4×** | median warm `read_pdf` latency vs the TypeScript engine — same host (linux-x64), 8 required fixture classes, median of class speedups ~15.4× |
| **~3.4× smaller** | clean install — 82.3 MiB → 24.4 MiB of `node_modules` vs TS 3.0.14 |
| **20 files** | on disk vs 4,101 — one native binary per platform, **zero** production JS dependencies |
| **5 platforms** | macOS arm64/x64 · Linux x64/arm64 · Windows x64 |

<sub>Warm-cache figure is **method-bounded**: long-lived MCP server, repeated *identical* local
`read_pdf` after warm-up, measured on the sole-Rust lineage (4.1.0) against TS 3.0.14. The first
request in a process pays full parse cost. No multi-host extrapolation.
See the [performance report](docs/specs/performance/4.1.0-same-host-performance-report.md) and
[claims policy](docs/specs/performance/4.1.0-performance-claims-policy.md).</sub>

## Platforms

One **optional** native package is selected for **your** host only:

| Platform | Native package |
| --- | --- |
| macOS arm64 | `@sylphx/citra-darwin-arm64` |
| macOS x64 | `@sylphx/citra-darwin-x64` |
| Linux x64 | `@sylphx/citra-linux-x64-gnu` |
| Linux arm64 | `@sylphx/citra-linux-arm64-gnu` |
| Windows x64 | `@sylphx/citra-win32-x64-msvc` |

## Security & trust

- **Local-first** — no required cloud provider; the PDF is not uploaded.
- **Fail closed** — a missing native binary stops the process; there is no silent TypeScript fallback.
- **Panic-unwind** — a malformed document (e.g. a broken ToUnicode CMap) fails the request, never the process ([#608](https://github.com/SylphxAI/citra/issues/608)).
- **HTTP transport is opt-in and hardened** — loopback by default, `MCP_API_KEY` enforced before binding elsewhere, and `--allow-dir` restricts filesystem reach. Details: [security docs](https://sylphxai.github.io/citra/security/maintainer-process) · report privately per [SECURITY.md](SECURITY.md).

## Documentation

| | |
| --- | --- |
| 🌐 **Website** | [sylphxai.github.io/citra](https://sylphxai.github.io/citra/) |
| ⚡ **Quickstart** | [Getting started](https://sylphxai.github.io/citra/guide/getting-started) |
| 📐 **API reference** | [docs/api](https://sylphxai.github.io/citra/api/) |
| 📐 **Evidence contract** | [What "proof" means](docs/EVIDENCE_CONTRACT.md) |
| 📊 **Performance** | [Method & results](https://sylphxai.github.io/citra/performance/) |
| ⚖️ **Comparison** | [Why not the alternatives](https://sylphxai.github.io/citra/comparison/) |

---

<div align="center">

**Stop PDF hallucinations. Give agents proof.**

```bash
npx -y @sylphx/citra
```

[⭐ **Star this repo**](https://github.com/SylphxAI/citra/stargazers) if Citra made your agent tell the truth.

</div>
