# Examples

Requests an AI agent sends to anymd to read, search, check, and cite
documents. anymd lists three MCP tools: `read`, `search`, and `inspect`.

## Requests

| File | What it shows |
| --- | --- |
| [`read-basic.json`](./read-basic.json) | `read` with only `source`: a document as Markdown |
| [`read-pages.json`](./read-pages.json) | `read` with `pages`, `max_tokens`, and `cursor` |
| [`search-then-verify.json`](./search-then-verify.json) | `search` → `inspect` (`render_page`) |
| [`evidence-crop.json`](./evidence-crop.json) | `inspect` (`extract_regions`): a region crop for citation |
| [`ocr-scanned.json`](./ocr-scanned.json) | `inspect` (`ocr_pages`): OCR for scanned PDFs |
| [`inspect-structure.json`](./inspect-structure.json) | `inspect` (`structure`): structured JSON with geometry and reports |

## MCP client setup

`npx -y @sylphx/anymd setup` adds anymd to every MCP client on the machine.
To add it by hand, every client runs the same command.

### Claude Code

```bash
claude mcp add anymd -- npx -y @sylphx/anymd
```

### Claude Desktop, Cursor, Windsurf, Cline

```json
{
  "mcpServers": {
    "anymd": {
      "command": "npx",
      "args": ["-y", "@sylphx/anymd"]
    }
  }
}
```

### VS Code

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "anymd": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@sylphx/anymd"]
    }
  }
}
```

### HTTP transport

```bash
MCP_TRANSPORT=http MCP_API_KEY=your-secret npx -y @sylphx/anymd
```

Then connect an MCP client to `http://127.0.0.1:8080/mcp` with the header
`X-API-Key: your-secret`. `MCP_HTTP_PORT` changes the port.

## Agent workflows

### Read first

```
Agent → read(source) → Markdown with <!-- page N --> markers
Agent → answers the question and cites page numbers
Agent → read(source, cursor) → the next part, when the answer ended with a cursor
```

### Search, then check

```
Agent → search(query, sources) → hits with file, page, and snippet
Agent → inspect(operation: render_page, sources[].pages) → the page as an image
Agent → inspect(operation: extract_regions, sources[].regions) → a crop of the exact evidence
```

Search first, and spend context only on the pages that matter.

### Check trust before citing

```
Agent → inspect(operation: structure, profile: research) → JSON with trust and accessibility reports
Agent → reviews trust warnings (hidden text, prompt-injection-like content)
Agent → cites the content or flags it as untrusted
```

### Scanned documents

```
Agent → read(source, ocr: true) → Markdown from a local tesseract
Agent → inspect(operation: ocr_pages) → OCR with word boxes from the configured provider
Agent → inspect(operation: analyze_regions, sources[].regions) → table, formula, and chart details
```
