# Formats

One `read` call handles every format below, detected from the file's bytes, not its name. Everything is parsed natively in Rust (zip/XML, calamine, html5ever): no Python, LibreOffice, or cloud service.

| Input | What you get |
|---|---|
| [PDF](#pdf) | Reading-order Markdown with headings, lists, tables, and page anchors |
| [Word](#word) `.docx` | Headings, formatting, links, lists, tables, footnotes, equations, tracked changes and comments |
| [PowerPoint](#powerpoint) `.pptx` | One section per slide, with notes and chart data |
| [Excel](#spreadsheets) `.xlsx .xls .ods` · CSV/TSV | One table per sheet |
| [EPUB](#epub) | One section per chapter |
| [Embedded images](#embedded-images) | Figures and pictures inside PDF, DOCX, PPTX, EPUB saved as files and marked in place |
| [HTML and URLs](#html-and-urls) | The main article only |
| [Markdown, text, JSON](#markdown-text-json) | Unchanged, with pagination |
| [Images](#images) | Metadata, EXIF, and OCR text |
| [Audio / video](#audio-and-video) | Metadata, chapters, subtitles, and transcripts |

## PDF

Reading-order Markdown: headings, paragraphs, lists, tables, sub/superscripts, `<!-- page N -->` markers, and bookmarks as an outline. Running headers and page numbers are removed. Image-only pages are OCR'd when `tesseract` is installed: pages are read at 300 dpi, several at a time, and the words tesseract finds are laid out like a text page, so scans get paragraphs and tables too.

How it works: anymd reads glyph positions rather than text runs. Glyphs are grouped into lines by baseline, which tolerates super- and subscripts. Word spaces come from the gaps between glyphs, measured against the font size and adjusted for letter tracking. A column-aware XY cut finds gutters between running text. Tables come from drawn lines where a table has them (a missing line between two cells makes a merged cell) and from aligned columns of whitespace where it does not. Wrapped cell text stays in its cell, stacked header lines become one header, and a header over several columns is kept with each of them. Text a reader cannot see (invisible text, or text in the colour of the box behind it) is left out. Pages are processed in parallel and isolated from each other, so one malformed page never fails the whole document.

`pages` selects PDF pages. For images, geometry, or JSON structure, use [`inspect`](./tools#inspect).

## Word

Headings, bold/italic, links, nested lists, tables with merged cells, footnotes, and equations as LaTeX. Pictures are [exported as image files](#embedded-images).

Tracked changes and comments become [CriticMarkup](https://github.com/CriticMarkup/CriticMarkup-toolkit), in the body, tables, text boxes, and footnotes:

| In Word | In the Markdown |
|---|---|
| Inserted text, or moved text at its new place | `{++new++}{>>Ana Lima (2026-09-29T14:05:00Z)<<}` |
| Deleted text, or moved text at its old place | `{--old--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}` |
| Deleted text next to inserted text | `{~~old~>new~~}{>>Ana Lima (2026-09-29T14:05:00Z)<<}` |
| A comment on some text | `{==text==}{>>Ana Lima (2026-09-30T08:15:00Z): comment<<}` |
| A comment on a point | `{>>Ana Lima (2026-09-30T08:15:00Z): comment<<}` |
| An inserted or deleted paragraph break | `{++` or `{--` around the blank line between the paragraphs, then its author and date |

Every tracked change is followed by who made it and when, as a comment, which is how CriticMarkup tracks several authors. A substitution made by two people names both, the deletion's author first. Neighbouring changes join into one only when the same person made them at the same time. Tracked changes inside a comment's own text keep their marks but not their author, since a comment cannot hold another comment. A change inside a link's text or a bold or italic run stays inside it (`[the {++new ++}page](url)`), and a footnote whose reference was inserted or deleted is marked the same way, label and all.

The author and date of changes and comments come from `w:author` and `w:date`. The date is copied exactly as stored, never converted; Word writes the author's local time there even though it ends in `Z`. Comments on the same text follow it in the order their anchors appear in the document, which is where Word puts a reply after the comment it answers. Formatting-only changes are not shown. In a document with tracked changes or comments, text that happens to contain a CriticMarkup delimiter is escaped with a backslash (`{\++`), so it reads the same but opens no span. In equations and image descriptions a space goes inside the delimiter instead (`-- }`), which LaTeX ignores. A document with no tracked changes and no comments is written exactly as before, with nothing escaped.

### Accept or reject every change

`revisions` (CLI `--revisions`) picks how tracked changes come out:

| `revisions` | What you get |
|---|---|
| `markup` (default) | The CriticMarkup above: every change, who made it and when, and the comments |
| `accept` | The text as Word shows it after Accept All, with no markup and no comments |
| `reject` | The text as Word shows it after Reject All, with no markup and no comments |

`accept` and `reject` apply to the body, tables (inserted or deleted rows and cells are kept or dropped), text boxes and footnotes. A paragraph break that goes away joins its paragraph with the next one, which keeps the next one's style, as in Word. `reject` also puts back formatting that a tracked change replaced. A document with no tracked changes and no comments reads the same under all three.

## PowerPoint

One section per slide in deck order: titles, bullets, tables, chart data, speaker notes, and pictures ([exported as image files](#embedded-images)). `pages` selects slides.

## Spreadsheets

`.xlsx`, `.xls`, `.ods`, CSV, and TSV. One Markdown table per sheet, dates as ISO strings, capped at 2,000 rows per sheet. `pages` selects sheets.

## EPUB

One section per chapter in spine order, plus title and author, with images [exported as files](#embedded-images). `pages` selects chapters.

## HTML and URLs

The main article only: navigation, cookie banners, and sidebars are dropped. Relative links are resolved, and code blocks keep their language. URL fetches are guarded; see [Security](./security).

## Markdown, text, JSON

Returned unchanged, with pagination and the token budget.

## Embedded images

An agent can open a standalone image file itself, but not one inside a container. `read` therefore exports the raster images embedded in PDF figures, DOCX and PPTX pictures and EPUB images (`images: "refs"`, the default; CLI `--images refs`), and marks where each sits in reading order:

```markdown
![Figure 1: Quarterly revenue by region](/home/me/.cache/anymd/images/9f2c1a7be03d55a4.png)
<!-- image: 240x150, page 1 -->
```

- **Files.** Each image is written once to `<cache>/images/<first 16 hex of its SHA-256>.<ext>`, where `<cache>` is `$ANYMD_CACHE_DIR`, else `~/.cache/anymd`, `~/Library/Caches/anymd` or `%LOCALAPPDATA%\anymd\cache`. The same picture is one file however often it is read. Files unused for 30 days are deleted (reading a document touches the files it uses), and the folder is kept under 2 GiB, oldest first; this runs at most once a day, at CLI and server start. Nothing is written beside the source document, and an image over 50 megapixels is refused.
- **Captions.** In a PDF, the nearest line directly below or above the image (within about half an inch) that starts with `Figure`, `Fig.`, `Table`, `圖`, `图` or `表` and a number. Otherwise the alt text (DOCX and PPTX `descr`, EPUB `alt`), otherwise `image`.
- **Decoration is skipped.** Images smaller than 48 x 48 px, PDF images under 2% of the page, and a picture that appears on three or more pages, slides or chapters (logos, running headers, ornaments).
- **A scanned page is not a figure.** A PDF page with no usable text layer and one image over at least 80% of it is the page itself: it gets no image ref and goes to OCR as before.
- **Which images.** PDF image XObjects (JPEG as is; 8-bit gray, RGB and palette rasters as PNG), DOCX `word/media`, PPTX slide pictures in slide order, and EPUB `<img>` files. The comment says `page N` for a PDF, `slide N` for a deck and `chapter N` for a book.
- **Budget.** A reference costs only its own two lines against `max_tokens`; the image itself is never counted.
- `inspect` with `operation: "structure"` also lists a local PDF's images as `embeddedImages`: page, bounding box in points (origin bottom left), size, caption and cached path.

Known gap in this version: figures drawn as vector graphics (charts, diagrams made of lines and text), inline images, CMYK, JPEG 2000, JBIG2 and CCITT images, and soft-mask transparency are not exported.

## Images

Dimensions and EXIF (camera, date, GPS), plus OCR text when `tesseract` is installed.

## Audio and video

Duration, streams, chapters, and embedded and sidecar subtitles (SRT/VTT), via `ffprobe`/`ffmpeg`. With `transcript: true` (CLI: `--transcript`), a local Qwen3-ASR transcript.

### Transcripts

`transcript: true` (CLI: `--transcript`) uses bundled **transcribe-cpp 0.2.4** and
**Qwen3-ASR-1.7B Q8_0 for every language**, with automatic language detection.
Whisper is no longer an engine option. Audio and documents stay on your machine.

Install **ffmpeg** to extract audio. With `download_asr_model: true` (CLI:
`--download-asr-model`), anymd fetches
`Qwen3-ASR-1.7B-Q8_0.gguf` (2.19 GB), checks its exact size and pinned SHA-256,
and atomically saves it in `$ANYMD_CACHE_DIR/models`, or the platform cache
(`~/.cache/anymd/models`, `~/Library/Caches/anymd/models`, or
`%LOCALAPPDATA%\anymd\cache\models`). Later requests verify and reuse it.
`ANYMD_ASR_MODEL` can point at a preinstalled copy of **the same pinned model**;
other models and corrupt cached files are rejected, not silently substituted.
The pinned download revision and checksum are in
[`asr.rs`](https://github.com/SylphxAI/anymd/blob/main/crates/anymd-formats/src/asr.rs).
To work offline, install that exact model first. `transcript: true` alone never
downloads models; missing weights produce an installation hint. No weights are
fetched for ordinary metadata or subtitle reads.

Long audio is decoded one 20-second chunk at a time, keeping audio memory bounded
and all timestamps on the original source timeline. The model is loaded once per
transcript. Segment timestamps mark chunk boundaries; they are not estimated word
positions. Digitally silent chunks are skipped. An inference error or truncation
is reported rather than presented as a complete transcript.

For **word timestamps**, an optional [CrispASR 0.8.38](https://github.com/CrispStrobe/CrispASR/releases/tag/v0.8.38)
`crispasr` binary on PATH (or `ANYMD_ALIGNER_BIN`) runs **standalone alignment only**
with Qwen3-ForcedAligner-0.6B Q8_0. It does not run ASR or replace transcribe-cpp.
The aligner weights (986 MB) are also revision/size/SHA-256 pinned and fetched on
first supported use only when `download_asr_model` is true;
`ANYMD_ALIGNER_MODEL` accepts a preinstalled pinned copy.
Supported language codes are en, zh, yue, ja, ko, fr, de, it, pt, ru and es.
Where that optional runtime is unavailable, the language is unsupported, or
alignment fails validation, anymd keeps the transcript and labels its timestamps
as **segment**, not word. Word timings must be monotonic, inside the chunk and
account for the transcript text. Output explicitly names word, segment or mixed
granularity and includes millisecond start/end times.

```bash
anymd talk.mp4 --download-asr-model  # explicitly allow the model download
anymd talk.mp4 --transcript          # cached/preinstalled models only
```

`download_asr_model: true` / `--download-asr-model` also implies `transcript`.
The old `download_whisper_model` / `--download-whisper-model` spelling is accepted
only as a compatibility alias for Qwen; no Whisper engine or weights remain.
Old `ANYMD_WHISPER_*` settings are not used.

**Japanese trade-off:** on our 200-utterance FLEURS sample, Qwen trails the
whisper-turbo benchmark control (5.93 vs 4.80 raw CER; 5.56 vs 4.59 with
symmetric numeral/kana normalisation). We use one speech model
for all languages and accept this gap. See [ASR benchmarks](benchmarks.md#speech-to-text).

## Optional tools

anymd never needs these, but uses them when they are on your `PATH`:

| Tool | Adds |
|---|---|
| `tesseract` | OCR for images and scanned PDF pages |
| `ffprobe` | Audio/video metadata and chapters |
| `ffmpeg` | Embedded subtitles and transcript audio |
| `crispasr` (optional, alignment only) | Qwen3-ForcedAligner word timestamps (see [Transcripts](#transcripts)) |

Check what anymd found:

```bash
$ anymd doctor
anymd 6.0.0 (native Rust)
  tesseract    found      OCR for images and scanned PDF pages
  ffprobe      found      audio/video metadata and chapters
  ffmpeg       found      embedded subtitles and transcript audio
Transcripts (--transcript):
  ASR runtime    transcribe-cpp 0.2.4 (CPU, bundled)
  ASR model      not installed: Qwen3-ASR-1.7B-Q8_0.gguf (2185 MB, SHA-256 pinned)
```

Typical installs: `brew install tesseract ffmpeg` on macOS, `apt install tesseract-ocr ffmpeg` on Debian/Ubuntu. For other OCR languages, install the tesseract language pack (for example `tesseract-ocr-chi-tra`).
