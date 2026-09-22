# Stop Your Agent From Hallucinating on PDFs


Plain-text PDF tools make agents invent citations. **PDF Reader MCP gives them evidence** — page numbers, table cells, crops, and provenance.

Install: `npm install -g @sylphx/citra`

You asked Claude to summarize a 40-page contract. It cited page 12. Page 12
does not say what it claimed.

That is not a model failure. It is a **PDF workflow failure**.

## What actually went wrong

Most agent stacks treat PDFs like `.txt` files with extra steps:

1. Extract text.
2. Dump it into context.
3. Ask the model to answer.

That pipeline loses tables, skips scanned pages, scrambles multi-column layout,
and never tells the agent that content is missing. The model fills the gaps
with fluent guesses.

## Three failures you cannot fix with a better prompt

| Failure | What the agent sees | What actually happened |
| --- | --- | --- |
| Table flattening | A paragraph of numbers | Row/column structure was destroyed |
| Scanned pages | Empty or sparse text | The page was an image, not text |
| Hidden text | "Extra" content in context | Invisible PDF text layer the human never saw |

No system prompt fixes missing evidence.

## What evidence-first reading changes

PDF Reader MCP does not stop at text extraction. One `read_pdf` call returns an
**structured document result**:

- **Markdown and chunks** the agent can read.
- **Tables with cell geometry** so structure survives.
- **Document map and AST** so headings, lists, and captions keep their roles.
- **Trust report** so hidden or risky content is flagged, not silently trusted.
- **Page + bounding box provenance** so answers can link back to source regions.
- **`search_pdf` → `pdf_evidence`** so the agent can verify before citing.

The agent still uses a language model. The difference is it works from **claims
with coordinates**, not from a lossy text dump.

## Try the fix in 30 seconds

```bash
claude mcp add pdf-reader -- npx @sylphx/citra
```

```json
{
  "sources": [{ "path": "/absolute/path/to/your.pdf" }]
}
```

No flags. No provider setup for digital-text PDFs. Inspect the `auto_read`
block to see what route was chosen, then read markdown, tables, and trust
signals in one response.

## When you need visual proof

```json
{
  "sources": [{ "path": "/absolute/path/to/your.pdf" }],
  "query": "termination clause",
  "max_matches_per_source": 5
}
```

Take the match page and bounding box into `pdf_evidence` (`render_page` or
`extract_regions`) before the agent cites or summarizes.

## Why this project exists

Agents are moving from chat to **work**: contracts, filings, specs, medical
records, research PDFs. The cost of a wrong citation is no longer "sounds a
bit off" — it is bad decisions on real documents.

PDF Reader MCP is built for that shift: local-first, MCP-native, benchmark-gated,
and designed so agents can **prove** what they read.

If that matches the stack you are building:

- [Get started](../guide/getting-started.md)
- [See capability comparison](../comparison/index.md)
- [⭐ Star the repo](https://github.com/SylphxAI/citra) so the next
  builder finds it before they ship another plain-text PDF dump