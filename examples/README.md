# Examples

Real-world workflows showing how AI agents use PDF Reader MCP to read, search,
verify, and cite PDF documents with evidence.

## Quick Examples

| File | What it shows |
| --- | --- |
| [`read-pdf-basic.json`](./read-pdf-basic.json) | One-call smart read: `read_pdf` with only `sources` |
| [`read-pdf-options.json`](./read-pdf-options.json) | Manual extraction with `include_*` flags |
| [`search-then-verify.json`](./search-then-verify.json) | `search_pdf` → `pdf_evidence` workflow |
| [`evidence-crop.json`](./evidence-crop.json) | Extract a region crop for citation |
| [`ocr-scanned.json`](./ocr-scanned.json) | OCR path for scanned PDFs |
| [`agent-document-twin.json`](./agent-document-twin.json) | Full Agent Document Twin output shape |

## MCP Client Snippets

### Claude Code

```bash
claude mcp add pdf-reader -- npx @sylphx/pdf-reader-mcp
```

### Claude Desktop

```json
{
  "mcpServers": {
    "pdf-reader": {
      "command": "npx",
      "args": ["@sylphx/pdf-reader-mcp"]
    }
  }
}
```

### Cursor

```json
{
  "mcpServers": {
    "pdf-reader": {
      "command": "npx",
      "args": ["@sylphx/pdf-reader-mcp"]
    }
  }
}
```

### VS Code (Copilot Chat MCP)

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "pdf-reader": {
      "command": "npx",
      "args": ["@sylphx/pdf-reader-mcp"]
    }
  }
}
```

### Windsurf

```json
{
  "mcpServers": {
    "pdf-reader": {
      "command": "npx",
      "args": ["@sylphx/pdf-reader-mcp"]
    }
  }
}
```

### Cline

Add to `cline_mcp_settings.json`:

```json
{
  "mcpServers": {
    "pdf-reader": {
      "command": "npx",
      "args": ["@sylphx/pdf-reader-mcp"]
    }
  }
}
```

### Warp Terminal

```toml
[mcp.pdf-reader]
command = "npx"
args = ["@sylphx/pdf-reader-mcp"]
```

### HTTP Transport (Remote)

```bash
MCP_TRANSPORT=http MCP_API_KEY=your-secret npx @sylphx/pdf-reader-mcp
```

Then connect any MCP client to `http://127.0.0.1:3000/mcp` with header
`X-API-Key: your-secret`.

## Agent Workflow Patterns

### Pattern 1: Read First, Ask Questions Later

```
Agent → read_pdf(sources) → gets Agent Document Twin (markdown, chunks, tables, trust report)
Agent → uses the twin to answer the user's question
Agent → cites page numbers and evidence IDs from the twin
```

This is the default V3 path. One call, full document intelligence.

### Pattern 2: Search, Then Verify

```
Agent → search_pdf(sources, query) → gets literal matches with page/box provenance
Agent → pdf_evidence(operation: render_page, page) → visual proof of the match
Agent → pdf_evidence(operation: extract_regions, regions) → crops the exact evidence
```

Cheap search first, spend context only on relevant evidence.

### Pattern 3: Trust-Check Before Citing

```
Agent → read_pdf(sources) → fast twin (markdown, tables, geometry). Trust is profile research or auto true.
Agent → reviews trust warnings (hidden text, prompt-injection-like content)
Agent → decides whether to cite or flag as untrusted
```

Prevents agents from citing manipulated or unsafe PDF content.

### Pattern 4: Scanned Document Recovery

```
Agent → read_pdf(sources) → auto-routes scanned pages to OCR if provider is ready
Agent → pdf_evidence(operation: ocr_pages, pages) → explicit OCR with word boxes
Agent → pdf_evidence(operation: analyze_regions, regions) → table/formula/chart enrichment
```

Scanned pages get OCR provenance linked back to source renders.
