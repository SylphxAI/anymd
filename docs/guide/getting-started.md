# Quickstart — from PDF to proof

Ten minutes from install to a claim a human can check. The point is not
"extract text" — it is **never have to guess a page number again**.

::: tip New to the problem?
Read [Stop PDF hallucinations](/articles/stop-pdf-hallucinations) first — it
explains why a text dump makes agents wrong in a way that sounds confident.
:::

## 1. Install

```bash
npx -y @sylphx/citra
```

Add it to your host as shown in [Installation](/guide/installation). Verify
with `npx -y @sylphx/citra --help`.

## 2. Your first read

One call. Let Citra choose the extraction route:

```json
{
  "sources": [{ "path": "/absolute/path/to/report.pdf" }]
}
```

That is the whole request. With no `include_*` flags, `read_pdf` profiles the
document, picks high-value extraction options, and returns the **Agent Document
Twin**: text, tables, structure, and citations in one response.

Deepen it without learning every switch:

```json
{
  "sources": [{ "path": "/absolute/path/to/report.pdf" }],
  "auto_detail": "full"
}
```

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
3. **`gaps`** — when Citra cannot prove something, it names the gap instead of
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

> Read `/absolute/path/to/report.pdf` with Citra. Then answer my question and
> cite the page and — for numbers — the table and cell you took them from. If
> the document does not prove an answer, say so instead of guessing.

That last sentence is what makes the evidence path do its job.

## Where next

- [API reference](/api/) — every option and result field
- [The evidence contract](/EVIDENCE_CONTRACT) — what "proof" means here
- [Performance](/performance/) — how fast, and how that was measured
- [Comparison](/comparison/) — why not the alternatives
