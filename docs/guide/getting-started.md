# Quickstart — from PDF to proof

Ten minutes from install to a claim a human can check. The point is not
"extract text" — it is **never have to guess a page number again**.

::: tip New to the problem?
Read [Stop PDF hallucinations](/articles/stop-pdf-hallucinations) first — it
explains why a text dump makes agents wrong in a way that sounds confident.
:::

## 1. Install

```bash
npx -y @sylphx/anymd
```

Add it to your host as shown in [Installation](/guide/installation). Verify
with `npx -y @sylphx/anymd --help`.

## 2. Your first read

One call. The default is **fast** — markdown, tables, chunks, geometry, and
citations. No OCR, no trust audit, no sampling.

```json
{
  "sources": [{ "path": "/absolute/path/to/report.pdf" }]
}
```

A page filter still uses fast, and it reads only those pages:

```json
{
  "sources": [{ "path": "/absolute/path/to/report.pdf", "pages": [1, 2] }]
}
```

Ask for more by name:

```json
{ "sources": [{ "path": "/absolute/path/to/report.pdf" }], "profile": "quality" }
```

`quality` adds the text layer, HTML, elements, and the document AST. It still
does not OCR. `research` adds safety, trust, and accessibility on top of that.
`auto_detail` wins when both are set. `full` is the deepest preset and still
does not render or OCR.

OCR is a different tool:

```json
{ "sources": [{ "path": "/absolute/path/to/scan.pdf" }], "op": "ocr_pages" }
```

That is `pdf_evidence`, and it needs an OCR provider you configured. A missing
provider returns a gap, not a guessed transcript.

## 3. Read the evidence, not the prose

A real response (excerpt) against a two-page table fixture:

```json
{
  "route": { "engine": "rust-core", "path": "rust-read-pdf-v1" },
  "source": { "hash": "99d313eb…", "path": "…/selectable-table-v1.pdf" },
  "results": [{
    "data": {
      "table_info": [{
        "page": 1,
        "bounding_box": { "left": 72, "top": 151, "right": 454.8, "bottom": 79 },
        "colCount": 3,
        "cellCount": 9,
        "confidence": 0.92,
        "continuation": {
          "role": "starts",
          "groupId": "table-continuation-p1-table-1-p2-table-1",
          "signals": ["same_column_count", "repeated_header_candidate"]
        }
      }]
    }
  }],
  "gaps": []
}
```

Three things worth noticing:

1. **`page` + `bounding_box`** — the claim has a place in the document.
2. **`continuation`** — the table continues onto page 2 with matching columns.
   An agent that cites "the table" now knows to read the next page too.
3. **`gaps`** — when anymd cannot prove something, it names the gap instead of
   filling it. That is the whole difference.

## 4. Search first, read second

Reading everything is slow and expensive. Locate first:

```json
{
  "sources": [{ "path": "/absolute/path/to/report.pdf" }],
  "query": "revenue"
}
```

`search_pdf` returns page numbers, snippets, offsets and bounding-box
provenance — enough to decide *whether* to spend tokens on a deep read.

## 5. Verify before you claim

When an agent is about to assert a number, send it to the evidence tool:

```json
{
  "operation": "extract_regions",
  "sources": [{ "path": "/absolute/path/to/report.pdf" }],
  "regions": [{ "page": 1, "bounding_box": { "left": 72, "top": 151, "right": 454, "bottom": 79 } }]
}
```

Or render the page and look at it:

```json
{
  "operation": "render_page",
  "sources": [{ "path": "/absolute/path/to/report.pdf" }],
  "pages": [1]
}
```

`pdf_evidence` has five focused operations — `inspect`, `render_page`,
`extract_regions`, `ocr_pages`, `analyze_regions`. One tool, one `op` enum; no
near-duplicate vanity tools to learn.

## 6. Scanned or mixed documents

```json
{
  "sources": [{ "path": "/absolute/path/to/scanned.pdf" }],
  "pages": [1, 2, 3],
  "include_ocr_text_layer": true,
  "include_tables": true
}
```

OCR keeps its own provenance and confidence and is kept **separate** from
selectable text. OCR word boxes can feed table extraction, so a scanned table
still comes back with cells and geometry. Configure a provider per
[Installation](/guide/installation#optional-providers) — core reading never
needs one.

## 7. Trust signals — only when you ask

Hidden text, prompt-injection attempts, overlapping or spoofed content:

```json
{
  "sources": [{ "path": "/absolute/path/to/untrusted.pdf" }],
  "include_safety_findings": true,
  "include_trust_report": true
}
```

These are off by default: you pay for them when you need them.

## A prompt you can paste into your agent

> Read `/absolute/path/to/report.pdf` with anymd. Then answer my question and
> cite the page and — for numbers — the table and cell you took them from. If
> the document does not prove an answer, say so instead of guessing.

That last sentence is what makes the evidence path do its job.

## Where next

- [API reference](/api/) — every option and result field
- [The evidence contract](/EVIDENCE_CONTRACT) — what "proof" means here
- [Performance](/performance/) — how fast, and how that was measured
- [Comparison](/comparison/) — why not the alternatives
