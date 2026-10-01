# Check a PDF citation

`inspect` with `operation: "cite_check"` checks whether a quote occurs in extracted evidence inside a physical page and bounding box. It checks **quote/location support in extracted evidence; not semantic truth or independent confirmation of OCR accuracy**. A verified result is not proof that the statement is true, or that OCR read the source correctly.

No model judges the quote. Native PDF text is checked first. OCR runs only when you select an existing OCR engine; it never downloads a model automatically.

```json
{
  "operation": "cite_check",
  "sources": [{ "path": "report.pdf" }],
  "citations": [
    {
      "id": "revenue-line",
      "quote": "Total revenue",
      "page": 3,
      "bounding_box": { "left": 70, "bottom": 420, "right": 310, "top": 450 }
    }
  ]
}
```

## Inputs

- Exactly one PDF source: `{ "path": "report.pdf" }` or `{ "url": "https://example.org/report.pdf" }`. Use explicit URLs, not a URL in `path`. Local sources follow the server's allowed-directory policy, including symlinks. No `sources[].pages` or `regions`: each citation supplies its own location.
- `citations`: 1–100 entries. Each has optional `id`, required nonempty `quote`, one-based physical `page`, and `bounding_box`. Printed page labels are not page numbers.
- Each quote is at most 4,096 UTF-16 units (an astral character takes two); all quotes together are at most 64,000 units. At most 20 distinct pages are checked.
- Boxes use bottom-left PDF coordinates `{ left, bottom, right, top }`. All coordinates must be finite, with positive width and height. There is no padding option. The only containment tolerance is 0.0001 PDF unit, for coordinate rounding.
- `normalization`: `"none"` by default, or explicit `"whitespace_v1"`. Matching is always case-sensitive. `whitespace_v1` collapses Unicode whitespace runs to one ASCII space and trims boundaries. It does not change case, punctuation, ligatures, hyphenation or spelling. Exact matches take priority.
- `expected_source_sha256`: optional 64-digit hexadecimal SHA-256. A mismatch returns insufficient evidence without extraction or OCR. The same admitted snapshot is hashed, extracted and rendered.
- `ocr`: optional `"auto"`, `"vlm"` or `"tesseract"`, using the existing OCR owner and request permit. Omit it for native text only.
- `timeout_ms`: 1,000–300,000; default 60,000. One deadline covers source fetching, extraction and optional OCR. The native worker is supervised and terminated on expiry.
- `max_output_chars`: 1,000–1,000,000 UTF-16 units; default 200,000. If supporting locations exceed the output budget, the results become insufficient rather than returning truncated proof.

Sources are limited to 256 MiB. This operation adds no cache, provider or public tool; the server still exposes `outline`, `read`, `search` and `inspect`.

## Results

One result is returned per citation, in input order:

| Verdict | Meaning |
|---|---|
| `verified_exact` | The original quote occurs at the requested location with complete supporting geometry |
| `verified_normalized` | Exact matching failed, but explicit `whitespace_v1` matched with supported geometry |
| `unmatched` | Complete admissible evidence was checked and did not support the quote there |
| `insufficient_evidence` | The source hash, text coverage, geometry or requested OCR cannot support a decision |

Supported locations include the observed original text, original reading-order UTF-16 start/end offsets, supporting boxes, and a geometry level. Mixed spans carry `geometry_level: "mixed"` and retain a `geometry_levels` label for each supporting box. Each result carries the source hash and physical page. The evidence layer identifies native text or the OCR provider, its model provenance and render evidence id. Presence is checked, not uniqueness: up to eight supported occurrences are returned per quote.

Native character positions remain labelled `char_estimated`, not exact glyph geometry. Whole text items use `text_item`; OCR words and regions keep coarse `ocr_word` or `ocr_region` labels. A coarse item's complete box must fit inside the citation box. Character-based support requires coverage for every contributing non-whitespace character; a union of only the available boxes is not enough. OCR boxes use the renderer's inverse affine transform, including rotation and CropBox origin, rather than division by scale.

An image-only scan without OCR, missing geometry, truncated extraction or OCR, and a coarse region that cannot establish containment are insufficient, not non-matches. A supported positive can stand even if unrelated evidence is incomplete. Page-local extraction failures do not erase supported quotes on other pages. Invalid arguments, denied access, a non-PDF or unusable source, and worker execution failures are tool errors.
