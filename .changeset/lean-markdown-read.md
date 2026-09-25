---
'@sylphx/anymd': major
---

read_pdf and search_pdf now answer in clean, compact Markdown by default.

- New PDF layout engine: word spaces inferred from glyph gaps (fixes "Thedominantsequence…" glued words), two-column reading order, headings, lists, sub/superscripts, running header/footer removal, and pipe tables.
- A sources-only read_pdf returns Markdown with `<!-- page N -->` markers and a small front-matter header instead of a 2 MB JSON envelope (Attention Is All You Need: 2.06 MB / 703k tokens → 42 KB / 11k tokens, 1.0 s → 0.1 s).
- Long documents stop at `max_tokens` (default 20000) and end with a `cursor` to continue; large documents with bookmarks get a short outline.
- search_pdf returns one line per hit: page number plus a snippet with the match in bold.
- The structured JSON (document map, elements, geometry, trust and accessibility reports) is still available: pass `profile` (fast, quality, research) or any `include_*` flag to read_pdf, or `detail: true` to search_pdf.
