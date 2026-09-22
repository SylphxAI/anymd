# Why Agents Need Evidence-First PDF Reading


> Product: `@sylphx/citra` — sole-Rust MCP server for evidence-first PDF intelligence.

AI agents that read PDFs face a fundamental problem: text extraction alone is
not enough. An agent that extracts text and answers questions is working blind.
It does not know where the text came from, whether it is trustworthy, whether a
table was missed, or whether a scanned page was silently skipped.

**Evidence-first PDF reading** solves this by treating every piece of extracted
content as a claim that must be backed by source-level proof: page numbers,
bounding boxes, region crops, confidence scores, and provenance metadata.

This article explains why this matters, what goes wrong without it, and how
PDF Reader MCP's V3 architecture makes evidence-first reading the default path.

## The Problem: Text Extraction Is a Lossy Guess

Standard PDF text extraction tools return a string of characters. That string
might be:

- **In the wrong reading order** — multi-column layouts, sidebars, and footnotes
  get jumbled into a single stream.
- **Missing tables** — tables become flattened rows of text with no cell
  boundaries, row/column structure, or header detection.
- **Missing scanned pages** — image-based pages return empty strings, and the
  agent has no idea content was lost.
- **Contaminated by hidden text** — some PDFs contain text that is invisible
  to humans but present in the text layer, including prompt-injection-like
  content.
- **Missing structure** — headings, lists, captions, and semantic roles are
  lost, so the agent cannot distinguish a title from a footnote.

When an agent answers a question from this lossy output, it is guessing. When
it cites a page number, the number might be wrong. When it summarizes a table,
the table might not exist in the extracted text at all.

## The Solution: structured document result

PDF Reader MCP's V3 architecture solves this with the **structured document result** —
a linked, source-backed representation of the PDF that includes:

1. **Document map** — a structured tree of pages, elements, headings,
   paragraphs, tables, images, and captions with stable IDs and bounding boxes.
2. **Semantic AST** — a reading-order-aware abstract syntax tree that preserves
   document hierarchy and semantic roles.
3. **Text layer** — direction-aware runs, lines, words, and characters with
   page-level ranges and estimated bounding boxes.
4. **Tables** — cells, rows, columns, geometry, confidence scores, quality
   warnings, and continuation hints — not flattened text.
5. **Trust report** — deterministic signals for hidden text, prompt-injection-
   like patterns, visual spoofing, unsafe links, and redaction.
6. **Accessibility report** — tagged-PDF coverage, structure trees, form labels,
   heading hierarchy, and page grades.
7. **Citation chunks** — page-based, semantic, and size-based chunks with
   provenance back to their source elements and bounding boxes.

Every claim in the twin links back to the source: the page, the element, the
bounding box, and the extraction engine that produced it.

## The V3 Workflow: One Smart Tool First

The V3 design principle is simple: **one smart tool first, focused evidence
only when needed**.

### Step 1: Read the PDF

```
Agent → read_pdf(sources) → structured document result
```

With no manual `include_*` flags, `read_pdf` profiles the PDF, chooses the best
extraction route, and returns the complete structured document result. The agent gets
markdown, chunks, tables, trust/accessibility routing, and the selected
extraction arguments — all in one call.

### Step 2: Search When You Have a Specific Query

```
Agent → search_pdf(sources, query) → literal matches with page/box provenance
```

When the task is a specific lookup, `search_pdf` returns source-backed matches
with snippets, offsets, and bounding boxes — before spending context on a broad
read.

### Step 3: Verify with Evidence When the Answer Needs Proof

```
Agent → pdf_evidence(operation, sources) → inspect, render, crop, OCR, or analyze
```

When the agent needs to prove a claim, `pdf_evidence` runs a focused operation:
inspect the PDF structure, render a page, crop a region, OCR a scanned page, or
analyze a visual region through a configured provider.

## Why This Design Works for Agents

### Lower Context Cost

A 3-tool surface (`read_pdf`, `search_pdf`, `pdf_evidence`) costs less agent
context than a 10-tool surface. The agent does not need to learn which of many
tools to call — it starts with `read_pdf` and only escalates to `pdf_evidence`
when it needs focused proof.

### Trust-Before-Cite

The trust report is built into the twin, not an afterthought. Before an agent
cites a PDF, it can check whether the cited text was hidden, off-page,
overlapping with other content, or matched a prompt-injection pattern. This
prevents agents from citing manipulated or unsafe content.

### Scanned Page Recovery

Scanned pages are not silently skipped. `read_pdf` detects them during
inspection and routes them to a configured OCR provider when one is available.
The OCR text layer is kept separate from selectable text, and every OCR word
links back to the rendered source page.

### Provider-Enabled, Not Provider-Bundled

Heavy OCR, vision, formula, chart, and table enrichment are not bundled into
the package. They are provider-enabled: the agent or operator configures a
local command, HTTP endpoint, Ollama, OpenAI-compatible API, LM Studio, or
llama.cpp. The package stays lightweight (4 runtime dependencies) and the
default install works without downloading any models.

## Evidence in Practice: A Concrete Example

Consider an agent reading a financial report:

1. **`read_pdf`** returns the structured document result. The twin includes a table on
   page 5 with 4 rows and 3 columns: Quarter, Revenue, Growth. The table has
   a confidence score of 0.95 and no quality warnings.

2. The agent answers: "Q4 revenue was $12.3M, a 15% increase from Q3."

3. The agent cites: page 5, table on page 5, chunk `chunk-0023`.

4. **Trust check**: the trust report shows no hidden text or prompt-injection
   signals on page 5. The citation is safe.

5. **Optional**: the agent calls `pdf_evidence` with `extract_regions` to crop
   the exact table region and include it as an image in its response.

Without evidence-first reading, step 1 might return the table as flattened
text with no page number, no row/column structure, and no confidence score. The
agent would still answer, but the answer would be unverifiable.

## Benchmark-Gated Releases

Every release is gated by reproducible benchmarks:

- **39/39 SOTA release gate checks** pass
- **69/69 deterministic quality checks** pass
- **Performance benchmarks** are measured and checked in
- **Provider certification** covers OCR, region analysis, and visual enrichment
- **Package smoke tests** verify the published tarball

This means the evidence-first guarantees are not marketing claims — they are
machine-checked on every release.

## Conclusion

Agents that read PDFs need more than text. They need to know where the text
came from, whether it is trustworthy, whether content was lost, and how to cite
it. Evidence-first PDF reading, implemented through the structured document result
architecture, makes this the default — not an afterthought.

PDF Reader MCP's V3 design delivers this through one smart tool (`read_pdf`),
one search tool (`search_pdf`), and one evidence tool (`pdf_evidence`) — keeping
context cost low while proving every claim.
