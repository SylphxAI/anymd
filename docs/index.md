---
layout: home

hero:
  name: Citra
  text: Give your AI agent eyes for PDFs — with proof.
  tagline: Local-first PDF intelligence your agent can defend. Structured text, tables, OCR and page-level citations from one call — no cloud API key, no Docker, no guesswork.
  image:
    src: /logo.svg
    alt: Citra — a citrus tile holding a document
  actions:
    - theme: brand
      text: Get started in 30 seconds
      link: /guide/installation
    - theme: alt
      text: Star on GitHub
      link: https://github.com/SylphxAI/citra
---

<div class="cit-section">
  <span class="cit-eyebrow">The difference</span>
  <h2 class="cit-h2">Plain text makes agents guess.<br />Evidence makes them right.</h2>
  <p class="cit-lead">A text dump drops the page number, the table grid and the region. Your agent fills the gap — and a confidently wrong answer costs more than <em>“I can’t tell.”</em> Citra returns the locators that let a human check the claim.</p>
  <div class="cit-compare" style="margin-top:28px">
    <div class="side">
      <h3>What a text dump says</h3>
      <p>“Revenue was about $12M.”</p>
    </div>
    <div class="side good">
      <h3>What Citra returns</h3>
      <p>page 1 · table <span class="cit-cite">p1-table-1</span> · 3 cols / 9 cells · bbox 72,151 → 454,79 · confidence 0.92</p>
    </div>
  </div>
</div>

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
          "missingCellCount": 1,
          "mergedCellCandidateCount": 5,
          "signals": ["missing_cells", "merged_cell_candidates"]
        }
      }]
    }
  }],
  "gaps": []
}
```

<p class="cit-fine">Excerpt of a <strong>real</strong> <code>read_pdf</code> response against <code>test/fixtures/differential/v3014-selectable-table-v1.pdf</code> (paths shortened). The table is detected <em>and</em> linked to its continuation on page 2 — and when Citra cannot prove something, it says so in <code>gaps</code> instead of guessing.</p>

<div class="cit-section">
  <span class="cit-eyebrow">How it works</span>
  <h2 class="cit-h2">Three steps from PDF to proof</h2>
  <div class="cit-steps" style="margin-top:26px">
    <div class="cit-step">
  <div class="n">Step 1</div>
  <h3>Add it to your agent</h3>
  <p>One <code>npx</code> line. A stdio MCP server starts locally on any agent or CLI — Claude, Cursor, VS Code, Codex.</p>
</div>
    <div class="cit-step">
  <div class="n">Step 2</div>
  <h3>Point at a PDF</h3>
  <p>A single <code>read_pdf</code> call profiles the document and returns the structured document result: text, tables, structure, and citations.</p>
</div>
    <div class="cit-step">
  <div class="n">Step 3</div>
  <h3>Cite instead of guess</h3>
  <p>Every claim carries page, geometry, and provenance — the evidence your agent can show a human.</p>
</div>
  </div>
</div>

<div class="cit-section">
  <span class="cit-eyebrow">What you get</span>
  <h2 class="cit-h2">Four tools. One surface.</h2>
  <p class="cit-lead">Few, powerful, obvious — no near-duplicate vanity tools. Advanced work lives behind one operation enum.</p>
  <div class="cit-grid three" style="margin-top:26px">
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" /> <path d="M14 2v5a1 1 0 0 0 1 1h5" /> <path d="M10 9H8" /> <path d="M16 13H8" /> <path d="M16 17H8" /></svg></div>
  <h3>read_pdf</h3>
  <p>The smart default. Markdown, tables with cells and geometry, structure, optional OCR, and citation-ready chunks.</p>
</div>
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21 21-4.34-4.34" /> <circle cx="11" cy="11" r="8" /></svg></div>
  <h3>search_pdf</h3>
  <p>Cheap literal retrieval first: page and bounding-box locators before you spend tokens on a deep read.</p>
</div>
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 3H5a2 2 0 0 0-2 2v14c0 1.1.9 2 2 2h3" /> <path d="M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3" /> <path d="M12 8v8" /> <path d="m9 13 3 3 3-3" /></svg></div>
  <h3>pdf_compare</h3>
  <p>Compare two local PDFs at page and term level without turning the whole document pair into context.</p>
</div>
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 2v14a2 2 0 0 0 2 2h14" /> <path d="M18 22V8a2 2 0 0 0-2-2H2" /></svg></div>
  <h3>pdf_evidence</h3>
  <p>Focused ops when a claim must be verified: inspect, render_page, extract_regions, ocr_pages, analyze_regions.</p>
</div>
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 9h18" /> <path d="M9 3v18" /> <rect x="3" y="3" width="18" height="18" rx="2" /></svg></div>
  <h3>Tables agents can trust</h3>
  <p>Rows, columns, cells and bounding boxes — plus quality signals that say when the grid is weak.</p>
</div>
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7V5a2 2 0 0 1 2-2h2" /> <path d="M17 3h2a2 2 0 0 1 2 2v2" /> <path d="M21 17v2a2 2 0 0 1-2 2h-2" /> <path d="M7 21H5a2 2 0 0 1-2-2v-2" /> <path d="M7 8h8" /> <path d="M7 12h10" /> <path d="M7 16h6" /></svg></div>
  <h3>Scanned PDFs that stay honest</h3>
  <p>OCR keeps its own provenance and confidence, separate from selectable text, linked to the rendered page.</p>
</div>
    <div class="cit-card">
  <div class="cit-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" /> <path d="m9 12 2 2 4-4" /></svg></div>
  <h3>Trust signals on request</h3>
  <p>Hidden text, prompt-injection, overlapping and spoofing findings — surfaced only when asked, with page-level routing.</p>
</div>
  </div>
</div>

<div class="cit-section">
  <span class="cit-eyebrow">Proof</span>
  <h2 class="cit-h2">Measured. Method-bounded.</h2>
  <div class="cit-proof" style="margin-top:26px">
    <div class="stat"><div class="num">≥ 10.4×</div><div class="lbl">median warm <code>read_pdf</code> latency vs the TypeScript engine — same host (linux-x64), 8 required fixture classes; median of class speedups ~15.4×</div></div>
    <div class="stat"><div class="num">~3.4×</div><div class="lbl">smaller clean install — 82.3 MiB → 24.4 MiB of <code>node_modules</code> vs TS 3.0.14</div></div>
    <div class="stat"><div class="num">20 files</div><div class="lbl">installed on disk vs 4,101 — one native binary per platform, zero production JS dependencies</div></div>
    <div class="stat"><div class="num">5 platforms</div><div class="lbl">macOS arm64/x64 · Linux x64/arm64 · Windows x64, each an optional native package</div></div>
  </div>
  <p class="cit-fine">Warm-cache figure is method-bounded: long-lived MCP server, repeated <em>identical</em> local <code>read_pdf</code> after warm-up, measured on the sole-Rust lineage (4.1.0) against TS 3.0.14. The first request in a process pays full parse cost. No multi-host extrapolation. Full method and evidence: <a href="./performance/">performance</a> · <a href="./benchmark">benchmark</a>.</p>
</div>

## Install in 30 seconds

```bash
npx -y @sylphx/citra
```

::: code-group
```json [Claude Desktop / Cursor / VS Code]
{
  "mcpServers": {
    "citra": { "command": "npx", "args": ["-y", "@sylphx/citra"] }
  }
}
```

```bash [Claude Code]
claude mcp add citra -- npx -y @sylphx/citra
```

```bash [Any agent / CLI]
npx -y @sylphx/citra
```
:::

<div class="cit-cta">
  <h2>Stop PDF hallucinations. Give agents proof.</h2>
  <p>Local-first by design. Five platform packages. One clean install. Fail-closed if the matching native binary is missing — never a silent fallback.</p>
  <p style="margin-top:18px"><a class="VPButton brand" href="./guide/installation">Read the quickstart</a> <a class="VPButton alt" href="https://github.com/SylphxAI/citra">Star the repo</a></p>
</div>
