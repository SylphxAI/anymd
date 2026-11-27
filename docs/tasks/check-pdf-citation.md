---
title: Check whether a PDF quote is on the cited page
description: Check a quote, physical PDF page and bounding box against extracted evidence. Runnable free search and Pro cite-check examples.
---

# Check whether a PDF quote is on the cited page

Free tools find the page a quote is on, return the box around it and save that
part of the page as a PNG. Anymd Pro checks whether the quote is on the cited
PDF page and inside the supplied box. It doesn't verify the claim
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

Expect the phrase in the extracted text and a search hit on page 1. Search gives
you the page; it does not return a box or validate a caller-supplied one.

## Free: crop the region behind the quote

`structure` returns every text element with its page and bounding box. Save it,
then print the elements that contain your quote:

```sh
node mcp-call.mjs inspect '{"operation":"structure","sources":[{"path":"citation.pdf","pages":[1]}]}' > structure.json
node -e '
const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
for (const s of JSON.parse(r.content[0].text).results)
  for (const e of s.data.elements)
    if (String(e.content ?? "").includes(process.argv[2])) console.log(e.page, JSON.stringify(e.bounding_box));
' structure.json "Deterministic quote"
```

Expect:

```text
1 {"bottom":700,"left":50,"right":152.696,"top":712}
```

The box covers the whole text element, such as a line or a paragraph, so it can
be wider than the quoted words. Coordinates are PDF points from the bottom-left
corner of the page.

Pass the page and box to `extract_regions`. `padding` adds a margin in PDF
points; `scale` sets the render resolution.

```sh
node mcp-call.mjs inspect '{"operation":"extract_regions","sources":[{"path":"citation.pdf","regions":[{"id":"quote","page":1,"bounding_box":{"left":50,"bottom":700,"right":152.696,"top":712},"padding":4}]}],"include_image":true,"scale":2}'
mv frame-1.png quote-crop.png
```

Expect a 222 × 40 PNG showing “Deterministic quote”. The client saves each
returned image as `frame-1.png`, `frame-2.png` and so on, replacing files from
earlier runs, so rename the ones you keep. The JSON lists `region_id`, `page`,
`source_bounding_box`, `crop_pixels` and the renderer, so you can show which
part of which page the image came from.

`crop_pixels` is the padded box multiplied by `scale`, measured from the top-left
of the rendered page: here `left` 92, `top` 152, `width` 222, `height` 40.

| On your own PDF | What to do |
| --- | --- |
| The page number printed on the page differs from the PDF page | Use the physical page from `search`, counting from 1 |
| Several elements contain the quote | Crop each one, or narrow the quote |
| No element contains the quote | The page may be a scan with no native text. Look at it with `render_page`, or run `ocr_pages` |
| You need more pixels | Raise `scale` (up to 4); `max_pixels_per_page` caps the size |

The [tools reference](/docs/tools#inspect) lists every `inspect` operation and
its arguments.

## Free: check the Studio sample yourself

[Studio’s sample PDF](/studio?sample=pdf#studio-cite-heading) shows a recorded
Pro check: “Quarterly Field Report” on page 1, box `left` 27.75, `bottom` 729.75,
`right` 246.4334, `top` 751.7475. You can reproduce the file hash and the box for
free in the same directory:

```sh
curl -fsSLO https://anymd.sylphx.com/samples/sample.pdf
node -e 'console.log(require("crypto").createHash("sha256").update(require("fs").readFileSync("sample.pdf")).digest("hex"))'
node mcp-call.mjs inspect '{"operation":"structure","sources":[{"path":"sample.pdf","pages":[1]}]}' > sample-structure.json
node -e '
const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
for (const s of JSON.parse(r.content[0].text).results)
  for (const e of s.data.elements)
    if (String(e.content ?? "").includes(process.argv[2])) console.log(e.page, JSON.stringify(e.bounding_box));
' sample-structure.json "Quarterly Field Report"
```

Expect the hash `f1042ff682abe03d2fd508f4ed252e9e206a2ca69f826af76c375b991a848b03`
and `1 {"bottom":729.75,"left":27.75,"right":246.4334,"top":751.7475}`, the same
file and box as the Studio recording.

## Pro: check quote and location together

After activating Pro with `anymd login`, run:

```sh
node mcp-call.mjs inspect '{"operation":"cite_check","sources":[{"path":"citation.pdf"}],"citations":[{"id":"supported","quote":"Deterministic quote","page":1,"bounding_box":{"left":0,"bottom":0,"right":612,"top":792}},{"id":"wrong-location","quote":"Deterministic quote","page":1,"bounding_box":{"left":0,"bottom":0,"right":10,"top":10}}]}'
```

The first box deliberately covers the whole page: it tests page membership,
not a precise crop. Expect `verified_exact` for `supported` and `unmatched`
for `wrong-location`. The result keeps the source hash and supporting locations.
Native character geometry is labelled `char_estimated`, not exact glyph bounds.
For a real citation, supply the actual box you want to test. To test the crop
from the free steps, pass its padded box:
`{"left":46,"bottom":696,"right":156.696,"top":716}`.

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
