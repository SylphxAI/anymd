---
title: Check whether a PDF quote is on the cited page
description: Check a quote, physical PDF page and bounding box against extracted evidence. Runnable free search and Pro cite-check examples with anymd 8.5.5.
---

# Check whether a PDF quote is on the cited page

Finding a phrase in a PDF is not the same as checking its citation. anymd Pro's
`cite_check` operation tests whether a quote occurs in extracted evidence on
the specified physical page, inside a specified box. It checks quote and location
support, **not whether the statement is true**, and does not independently
confirm OCR accuracy.

Free reading, searching and PDF inspection stay MIT licensed and ungated.
[See current Pro pricing](https://sylphxai.github.io/anymd/pricing?utm_source=docs_check_pdf_citation).

## Make a PDF you can reproduce

Use Node.js 18+, npm, Python 3 and a POSIX shell. The fixture generator uses only
Python's standard library; no font files, model, OCR engine or API key are needed.

```sh
mkdir anymd-citations && cd anymd-citations
npm install --no-save @sylphx/anymd@8.5.5
curl -fsSLO https://sylphxai.github.io/anymd/examples/mcp-call.mjs
curl -fsSLO https://sylphxai.github.io/anymd/examples/make-citation-pdf.py
python3 make-citation-pdf.py
```

The [generator](https://sylphxai.github.io/anymd/examples/make-citation-pdf.py) writes `citation.pdf`, a one-page
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

With an [existing licence activated](/pro#activate), run:

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

The [cite-check reference](/guide/cite-check) covers request limits, optional
OCR, geometry granularity, hash checks and failure cases.

[See current Pro pricing](https://sylphxai.github.io/anymd/pricing?utm_source=docs_check_pdf_citation).
