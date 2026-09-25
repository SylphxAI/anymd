---
'@sylphx/anymd': minor
---

The same binary is now a CLI, like MarkItDown but faster: `anymd paper.pdf > paper.md` prints Markdown to stdout. It handles several files, URLs, directories (lists their readable files), and stdin (`cat deck.pptx | anymd -`). Options: `-p/--pages`, `-o/--output`, `--max-tokens` and `--cursor`, `--ocr`/`--no-ocr`, and `--transcript`. `anymd search "<query>" [paths...]` searches files and directories from the terminal, and `anymd doctor` reports which optional tools (tesseract, ffprobe, ffmpeg, whisper.cpp) it found. When run with no file arguments and a piped stdin (how MCP clients launch it), or as `anymd mcp`, it serves MCP over stdio as before.
