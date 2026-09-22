# Design Philosophy

Citra is designed as an structured document result engine for MCP clients. The
core design goal is to preserve source evidence and routing signals across text,
visual, semantic, trust, accessibility, OCR, and provider-enriched layers while
keeping one sole-Rust production engine local-first.

It is built on these core principles:

## 1. Performance First

- **Concurrent Processing** - Multiple PDF sources are processed in parallel
- **Efficient Parsing** - Uses the native Rust core for bounded PDF parsing,
  rendering, structure, and evidence extraction
- **Minimal Overhead** - Direct stdio by default, with an explicit optional
  authenticated HTTP transport
- **Batch Operations** - Process multiple files in a single request

## 2. Comprehensive Extraction

- **Text Extraction** - Full document or specific pages
- **Text Layer** - Optional direction-aware run, line, word, and character records with page-level ranges, estimated bounding boxes, provenance, and metadata coverage diagnostics
- **Agent Document Map** - One navigable contract linking pages, elements, text-layer and metadata coverage, chunks, layout diagnostics, safety findings, trust report routing and signal indexes, accessibility report routing and issue indexes, visual evidence routing, and page geometry
- **Document AST** - Optional semantic tree for page, section, paragraph, list item, caption, header, footer, table, and image traversal with cross-page section context and caption-to-evidence links
- **Trust Report** - Optional consolidated risk report for content safety, visual-spoofing, tiny/off-page text, layout uncertainty, sparse pages, table quality, external links, unsafe link schemes, selected-page counters, and redacted evidence snippets
- **Accessibility Report** - Optional deterministic report for tagged-PDF coverage, tag-to-visible-content coverage, structure trees, headings, images, forms, links, accessibility permissions, issue summaries, and page-grade routing
- **PDF Search Evidence** - Literal search over extracted text with snippets, match offsets, character-derived or text-item bounding boxes, and provenance
- **Visual Page Evidence** - Bounded page rendering with evidence IDs, provenance, and MCP image parts for OCR routing and page inspection
- **Region Crop Evidence** - Bbox-grounded visual crops that connect extracted structure back to focused source evidence
- **Visual Region Analysis** - Optional command or HTTP provider enrichment for focused table/image crops and caption-derived visual regions, normalized into table, chart, formula, figure, image-description, confidence, warning, and provenance fields
- **Configured OCR Text Layer** - Optional command-provider OCR over bounded rendered pages, normalized into text, confidence, word boxes, language, and provenance
- **Structured Elements** - Optional agent-ready elements with stable IDs, provenance, and best-effort bounding boxes
- **Semantic Hints** - Optional deterministic heading, list, paragraph, caption, header, and footer hints on text elements
- **Table Geometry** - Optional table elements include row data, cell metadata, confidence, and best-effort coordinates
- **Table Quality** - Optional table diagnostics expose completeness, missing cells, inferred merged-cell candidates, and repeated-header or page-edge geometry continuation candidates
- **Markdown Rendering** - Page-aware Markdown for RAG, summarization, and agent context
- **HTML Rendering** - Escaped page-aware HTML for preview, export, and conversion workflows
- **Citation Chunks** - Page, semantic, size, and table chunks with element IDs and best-effort bounding boxes
- **Document Signals** - Outlines, annotations, structure trees, page geometry, form fields, attachment metadata, page labels, permissions, and mark info
- **Content Safety Findings** - Optional deterministic warnings for prompt-injection patterns, tiny text, and off-page text
- **Page Ranges** - Flexible page selection with ranges like "1-5, 10, 15-20"
- **Metadata Access** - Document properties, author, title, dates
- **Image Extraction** - Embedded images as base64-encoded PNG

## 3. Simple Integration

- **Smart Tool Surface** - `read_pdf` is the default V3 entrypoint, `search_pdf` handles cheap literal evidence retrieval, and `pdf_evidence` consolidates inspect, render, crop, OCR, and visual-region analysis operations
- **Safe Provider Status** - Automatic reads and evidence inspection report optional-provider readiness and health metadata without exposing local provider paths or arguments
- **Standard MCP** - Compatible with any MCP client
- **Easy Setup** - One command installation via npx
- **Multiple Clients** - Works with Claude Desktop, Claude Code, Cursor, and more

## 4. Flexible Input

- **Local Files** - Read PDFs from any path on the filesystem
- **Remote URLs** - Download and process PDFs from URLs
- **Mixed Sources** - Combine local and remote files in one request

## 5. Robust Error Handling

- **Graceful Failures** - One failed source doesn't stop others
- **Clear Errors** - Specific error codes and messages
- **Partial Results** - Get results from successful sources even if some fail

## 6. Agent-Ready Output

- **Stable References** - Element IDs and page numbers make downstream citations easier to preserve
- **Tool Routing** - Automatic read summaries and evidence inspection expose executable arguments, missing inputs, and provider requirements so agents can choose the next MCP call without parsing prose
- **Text Fidelity** - Text layers expose run metadata, line IDs, word records, character records, character ranges, and bbox coverage without requiring agents to parse plain strings
- **Searchable Evidence** - Search matches carry snippets, offsets, character-derived or text-item boxes, and provenance so agents can decide when to read, crop, render, or cite
- **Document Map** - Pages, elements, text-layer and metadata coverage, chunks, layout diagnostics, safety findings, trust report routing and signal indexes, accessibility report routing and issue indexes, visual evidence routing, and geometry are linked from one response shape
- **Semantic Tree** - The document AST gives agents a hierarchy for traversal while keeping element IDs, chunk IDs, continued section context, and caption links as evidence anchors
- **Trust Routing** - The trust report turns hidden text, visual-spoofing, tiny/off-page text, layout, table, annotation, unsafe-link, selected-page category counts, and redacted snippet signals into page-level routing guidance
- **Accessibility Routing** - The accessibility report turns tagged structure, tag-to-visible-content coverage, headings, images, forms, links, permissions, issue types, severities, and page grades into page-level quality guidance
- **Semantic Hints** - Heading, list, paragraph, caption, header, and footer hints carry confidence and signals without overstating parser certainty
- **Caption Evidence** - Caption nodes link to nearby matching table, image, figure, chart, formula, and diagram nodes, while targets keep reverse caption IDs for agent traversal
- **Cell-Level Provenance** - Table cells can carry row/column indexes, header/span hints, inference flags, and coordinates for downstream citation workflows
- **Table Trust Signals** - Table quality warnings tell agents when to verify sparse, merged, irregular, or continuation-candidate tables with visual evidence
- **Retrieval-Ready Chunks** - Page, semantic, size, and table chunks carry source references without requiring a separate indexing pass
- **Portable Renderings** - Markdown and HTML renderers support different agent, preview, and export workflows from the same extraction pass
- **Layout Provenance** - Page geometry and best-effort bounding boxes make extracted content easier to trace back to source pages
- **Visual Evidence** - Rendered pages give agents a bounded way to inspect original page appearance without duplicating base64 in JSON
- **Focused Evidence** - Region crops let agents verify tables, figures, charts, formulas, annotations, and citations without carrying whole-page images
- **Region Enrichment** - Region analysis lets local table, chart, formula, and caption providers return normalized fields tied to crop evidence IDs, including caption-derived regions for vector-drawn formulas, charts, figures, and diagrams
- **OCR Provenance** - OCR text layers point back to the render evidence ID used as provider input, so scanned-page text remains tied to source pixels
- **Safety Findings** - Deterministic content warnings help agents treat risky PDF text as data, not instructions
- **Recursive Reading Order** - Distant same-line text is segmented, then conservative band and column segmentation improves common multi-column PDFs with spanning headers or footers
- **Structured JSON First** - Machine-readable summaries come before large text or image parts
- **Binary Discipline** - Image bytes are delivered as MCP image content, not duplicated into JSON summaries
- **Provider Boundaries** - Optional engines are enabled through explicit local provider configuration instead of bundled heavy dependencies or request-selected commands
- **Extensible Model** - The response model can grow toward headings, tables, citations, and richer layout without breaking existing callers

## Technical Stack

- **Runtime**: native Rust MCP server; Node.js 22+ is the thin npm launcher only
- **PDF parsing and rendering**: `pdf-extract`, `lopdf`, and `hayro`
- **Image encoding**: Rust `image` crate with PNG support
- **Schema validation**: Rust `schemars` and `serde`
- **MCP SDK**: Rust `rmcp`
- **Package build**: Bun for the thin launcher and SDK surfaces
