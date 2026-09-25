# Formats

One `read` call handles every format below, detected from the file's bytes, not its name. Everything is parsed natively in Rust (zip/XML, calamine, html5ever): no Python, LibreOffice, or cloud service.

| Input | What you get |
|---|---|
| [PDF](#pdf) | Reading-order Markdown with headings, lists, tables, and page anchors |
| [Word](#word) `.docx` | Headings, formatting, links, lists, tables, footnotes, equations |
| [PowerPoint](#powerpoint) `.pptx` | One section per slide, with notes and chart data |
| [Excel](#spreadsheets) `.xlsx .xls .ods` · CSV/TSV | One table per sheet |
| [EPUB](#epub) | One section per chapter |
| [HTML and URLs](#html-and-urls) | The main article only |
| [Markdown, text, JSON](#markdown-text-json) | Unchanged, with pagination |
| [Images](#images) | Metadata, EXIF, and OCR text |
| [Audio / video](#audio-and-video) | Metadata, chapters, subtitles, and transcripts |

## PDF

Reading-order Markdown: headings, paragraphs, lists, tables, sub/superscripts, `<!-- page N -->` markers, and bookmarks as an outline. Running headers and page numbers are removed. Image-only pages are OCR'd when `tesseract` is installed.

How it works: anymd reads glyph positions rather than text runs. Glyphs are grouped into lines by baseline, which tolerates super- and subscripts. Word spaces come from the gaps between glyphs, measured against the font size and adjusted for letter tracking. A column-aware XY cut finds gutters between running text. Rows whose cells line up become pipe tables. Pages are processed in parallel and isolated from each other, so one malformed page never fails the whole document.

`pages` selects PDF pages. For images, geometry, or JSON structure, use [`inspect`](./tools#inspect).

## Word

Headings, bold/italic, links, nested lists, tables with merged cells, footnotes, and equations as LaTeX.

## PowerPoint

One section per slide in deck order: titles, bullets, tables, chart data, and speaker notes. `pages` selects slides.

## Spreadsheets

`.xlsx`, `.xls`, `.ods`, CSV, and TSV. One Markdown table per sheet, dates as ISO strings, capped at 2,000 rows per sheet. `pages` selects sheets.

## EPUB

One section per chapter in spine order, plus title and author. `pages` selects chapters.

## HTML and URLs

The main article only: navigation, cookie banners, and sidebars are dropped. Relative links are resolved, and code blocks keep their language. URL fetches are guarded; see [Security](./security).

## Markdown, text, JSON

Returned unchanged, with pagination and the token budget.

## Images

Dimensions and EXIF (camera, date, GPS), plus OCR text when `tesseract` is installed.

## Audio and video

Duration, streams, chapters, and embedded and sidecar subtitles (SRT/VTT), via `ffprobe`/`ffmpeg`. With `transcript: true` (CLI: `--transcript`), a local whisper.cpp transcript.

## Optional tools

anymd never needs these, but uses them when they are on your `PATH`:

| Tool | Adds |
|---|---|
| `tesseract` | OCR for images and scanned PDF pages |
| `ffprobe` | Audio/video metadata and chapters |
| `ffmpeg` | Embedded subtitles and transcript audio |
| `whisper-cli` (whisper.cpp) | Local transcripts; set `ANYMD_WHISPER_MODEL` to a model file |

Check what anymd found:

```bash
$ anymd doctor
anymd 6.0.0 (native Rust)
  tesseract    found      OCR for images and scanned PDF pages
  ffprobe      found      audio/video metadata and chapters
  ffmpeg       found      embedded subtitles and transcript audio
  whisper-cli  not found  local transcripts (with ANYMD_WHISPER_MODEL)
```

Typical installs: `brew install tesseract ffmpeg whisper-cpp` on macOS, `apt install tesseract-ocr ffmpeg` on Debian/Ubuntu. For other OCR languages, install the tesseract language pack (for example `tesseract-ocr-chi-tra`).
