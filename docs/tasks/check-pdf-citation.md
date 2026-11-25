---
title: Check whether a PDF quote is on the cited page
description: Check a quote, physical PDF page and bounding box against extracted evidence. Runnable free search and Pro cite-check examples.
---

# Check whether a PDF quote is on the cited page

Free search finds candidate quotes. Anymd Pro checks whether the quote is on
the cited PDF page and inside the supplied box. It doesn't verify the claim
itself or independently confirm OCR accuracy.

> Sample PDF: “Deterministic quote” on page 1 → `verified_exact` in the page
> box, `unmatched` in the empty box.

**[See Anymd Pro — US$29 one-time](/pricing?entry_surface=task_check_pdf_citation)**

## Make a PDF you can reproduce

Use Node.js 18+, npm, Python 3 and a POSIX shell. The fixture generator uses only
Python's standard library; no font files, model, OCR engine or API key are needed.

```sh
mkdir anymd-citations && cd anymd-citations
npm install --no-save @sylphx/anymd
curl -fsSLO https://anymd.sylphx.com/examples/mcp-call.mjs
curl -fsSLO https://anymd.sylphx.com/examples/make-citation-pdf.py
python3 make-citation-pdf.py
```

The [generator](/examples/make-citation-pdf.py) writes `citation.pdf`, a one-page
612 × 792 PDF with native text `Deterministic quote` at (50, 700). The
[MCP client](/examples/mcp-call.mjs) starts the installed release, confines file
access to this directory and prints the tool result. It does not upload the PDF.

## Free: read and find a quote

```sh
node mcp-call.mjs read '{"source":"citation.pdf","ocr":false,"images":"none"}'
node mcp-call.mjs search '{"query":"Deterministic quote","sources":["citation.pdf"],"mode":"literal","case_sensitive":true}'
```

Expect the phrase in the extracted text and a search hit on page 1. Search helps
you find candidate evidence; it does not validate a caller-supplied bounding box.

## Pro: check quote and location together

After activating Pro with `anymd login`, run:

```sh
node mcp-call.mjs inspect '{"operation":"cite_check","sources":[{"path":"citation.pdf"}],"citations":[{"id":"supported","quote":"Deterministic quote","page":1,"bounding_box":{"left":0,"bottom":0,"right":612,"top":792}},{"id":"wrong-location","quote":"Deterministic quote","page":1,"bounding_box":{"left":0,"bottom":0,"right":10,"top":10}}]}'
```

The first box deliberately covers the whole page: it tests page membership,
not a precise crop. Expect `verified_exact` for `supported` and `unmatched`
for `wrong-location`. The result keeps the source hash and supporting locations.
Native character geometry is labelled `char_estimated`, not exact glyph bounds.
For a real citation, supply the actual box you want to test.

Without a licence, a valid request returns the Pro message and does no citation
work. The normal MCP text response is not a verified citation.

## Treat missing evidence differently from a non-match

| Verdict | What to do |
| --- | --- |
| `verified_exact` | Keep the original quote, source hash, page and supporting locations |
| `verified_normalized` | Keep the explicit normalization policy as well as the evidence |
| `unmatched` | Complete admissible evidence did not support the quote at that location |
| `insufficient_evidence` | Do not decide: resolve the source, extraction, OCR or geometry gap |

Matching is case-sensitive. The default performs no normalization; explicit
`whitespace_v1` only collapses whitespace and trims boundaries. It does not fix
punctuation, case, hyphenation or OCR mistakes. A scan without requested OCR is
insufficient evidence, not proof that the quote is absent.

Pages are one-based physical pages, not printed labels. Boxes use bottom-left
PDF coordinates. For a source you have already hashed, pass
`expected_source_sha256`; a mismatch returns insufficient evidence without
extraction. There can be several supported occurrences: presence is not uniqueness.

The [cite-check reference](/docs/cite-check) covers request limits, optional
OCR, geometry granularity, hash checks and failure cases.

[See Anymd Pro pricing](/pricing?entry_surface=task_check_pdf_citation).
