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

### Transcripts

A transcript needs three things on your machine; nothing is uploaded.

- **whisper.cpp**: `whisper-cli` (or `whisper-cpp`) on `PATH`, or `ANYMD_WHISPER_BIN` pointing at it. macOS: `brew install whisper-cpp`. Windows: `whisper-bin-x64.zip` from the [whisper.cpp releases](https://github.com/ggml-org/whisper.cpp/releases). Linux: build from source (`git clone https://github.com/ggml-org/whisper.cpp && cd whisper.cpp && cmake -B build && cmake --build build -j --config Release`, then put `build/bin/whisper-cli` on `PATH`), the release tarball, or Homebrew.
- **ffmpeg**, to extract the audio track.
- **A ggml model**. `ANYMD_WHISPER_MODEL` wins when set; otherwise anymd uses a `ggml-*.bin` in its cache (`$ANYMD_CACHE_DIR/models`, else `~/.cache/anymd/models`, `~/Library/Caches/anymd/models`, or `%LOCALAPPDATA%\anymd\cache\models`). To fetch one on first use, pass `download_whisper_model: true` (CLI: `--download-whisper-model`, which implies `--transcript`) or set `ANYMD_WHISPER_AUTO_DOWNLOAD=1`. anymd downloads `ggml-base.en.bin` (148 MB) from the official [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) repository, checks its SHA-256, and renames it into place. `ANYMD_WHISPER_MODEL_SIZE` picks `tiny`, `tiny.en`, `base`, `base.en`, `small`, or `small.en`; `ANYMD_WHISPER_MODEL_BASE_URL` points at a mirror (the hash is still checked). Multilingual models detect the spoken language.

```bash
anymd talk.mp4 --download-whisper-model
```

When a piece is missing, the output names it with the install command for your OS instead of failing.

## Optional tools

anymd never needs these, but uses them when they are on your `PATH`:

| Tool | Adds |
|---|---|
| `tesseract` | OCR for images and scanned PDF pages |
| `ffprobe` | Audio/video metadata and chapters |
| `ffmpeg` | Embedded subtitles and transcript audio |
| `whisper-cli` (whisper.cpp) | Local transcripts (see [Transcripts](#transcripts)) |

Check what anymd found:

```bash
$ anymd doctor
anymd 6.0.0 (native Rust)
  tesseract    found      OCR for images and scanned PDF pages
  ffprobe      found      audio/video metadata and chapters
  ffmpeg       found      embedded subtitles and transcript audio
Transcripts (--transcript):
  whisper.cpp    found      /opt/homebrew/bin/whisper-cli
  whisper model  not found  cache /Users/me/Library/Caches/anymd/models; `--download-whisper-model` fetches ggml-base.en.bin (148 MB)
```

Typical installs: `brew install tesseract ffmpeg whisper-cpp` on macOS, `apt install tesseract-ocr ffmpeg` on Debian/Ubuntu. For other OCR languages, install the tesseract language pack (for example `tesseract-ocr-chi-tra`).
