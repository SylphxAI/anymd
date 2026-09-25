---
'@sylphx/anymd': major
---

Read any document, not just PDFs, through three obvious tools.

- `read {source}`: a path, http(s) URL, or directory. Formats: PDF, DOCX, PPTX, XLSX/XLS/ODS, CSV/TSV, EPUB, HTML and web pages (main-content extraction, SSRF-guarded fetch), Markdown/text, images (metadata plus OCR with a local tesseract), audio/video (ffprobe metadata, chapters, embedded and sidecar subtitles, optional whisper.cpp transcript), and SRT/VTT. Pages, slides, sheets, and chapters get citation markers, and `pages`, `max_tokens`, and `cursor` work for every format. Image-only PDF pages are OCR'd automatically when tesseract is installed.
- `search {query, sources}`: files, directories (recursive, .gitignore aware, `glob` filter), and URLs across every format. `mode` auto finds the exact phrase and falls back to BM25-ranked passages when there is none.
- `inspect {operation, sources}`: the PDF deep dive. It renders, crops, runs OCR or provider analysis, returns structured JSON (`structure`), and diffs two PDFs (`compare`).
- `read_pdf`, `search_pdf`, `pdf_evidence`, and `pdf_compare` still work under their old names for this major version, but tools/list no longer shows them.
