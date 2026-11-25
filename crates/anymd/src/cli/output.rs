use std::io::Write;

use crate::document::EvidenceKind;

pub const USAGE: &str = "\
anymd — any file → clean Markdown for AI agents

Usage:
  anymd <file|url|dir>... [options]   Convert to Markdown on stdout
  anymd - [options]                   Convert stdin
  anymd outline <file|url> [--format tree|json]  Show the heading tree
  anymd search <query> [path|url...]  Search files and directories (default: .)
  anymd cite-check <pdf> <quote>... [-p <page>]  Verify PDF quote/page citations (anymd Pro)
  anymd mcp [--allow-dir=<path>]...   Run the MCP server on stdio
                                      (also the default when stdin is piped and no file is given)
  anymd login                         Sign in and sync your offline Pro licence
  anymd setup [--dry-run] [--remove]  Add anymd to the MCP clients on this machine
                                      (Claude Code, Codex, Cursor, VS Code, Claude Desktop,
                                      Windsurf, Gemini CLI); --remove undoes it
  anymd pro status | buy | activate <token> Show, buy or activate an anymd Pro licence ($29 once)
  anymd doctor                        Print version and optional tool availability
  anymd version                       Print the version

Outline options:
      --format <mode>      tree (CLI default) or json; --json also selects JSON
                           --ocr, --images and --revisions match node read options

Read options:
      --node <id>          Read a node from outline; repeat it with a cursor
  -p, --pages <spec>       Pages, slides, sheets, or chapters, e.g. 1-5,8
  -o, --output <file>      Write to a file instead of stdout
      --max-tokens <n>     Stop at a token budget and print a cursor
      --cursor <cursor>    Continue from a cursor
      --ocr [auto|vlm|tesseract]  Select local OCR; --no-ocr disables it
      (ANYMD_OCR selects the default; auto never downloads weights)
      --transcript         Transcribe audio/video locally with Qwen3-ASR-1.7B Q8
      --download-asr-model
                           With --transcript (implied): download the pinned Qwen3 model on first use
                           (Q8, ~2.2 GB; --transcript alone never downloads)
      --images <mode>      refs (default): save images embedded in PDF/DOCX/PPTX/EPUB files to the
                           anymd cache and mark them in the Markdown; none: leave them out
      --revisions <mode>   Word tracked changes and comments: markup (default) as CriticMarkup,
                           accept or reject for the text with every change accepted or rejected
      --front-matter       Print the source/title/pages header (always on for several inputs)

Search options:
      --mode <m>           auto (default), literal, or ranked (BM25)
      --glob <glob>        Only files matching the glob, e.g. '*.pdf'
      --max <n>            Maximum hits (default 20)
  -i, --case-sensitive     Match case
  -w, --whole-word         Match whole words

  -h, --help               Show this help
  -V, --version            Show the version
      --                   Treat all following arguments as input paths

Formats: PDF, DOCX, PPTX, XLSX/XLS/ODS, CSV/TSV, EPUB, HTML and web URLs, Markdown/text,
images (metadata + OCR), audio/video (metadata, chapters, subtitles), SRT/VTT.";

pub(super) struct CommandOutput {
    pub(super) text: String,
    pub(super) failed: bool,
    pub(super) evidence: Option<EvidenceKind>,
}

pub(super) fn write(text: &str, failed: bool, path: Option<&str>) -> Result<(), String> {
    match path {
        Some(path) if failed => Err(format!("not writing {path}: conversion failed")),
        Some(path) => {
            std::fs::write(path, text.as_bytes()).map_err(|error| format!("{path}: {error}"))
        }
        None => std::io::stdout()
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string()),
    }
}

pub(super) fn error_line(front_matter: &str) -> String {
    if front_matter.trim_end() == crate::ocr_vlm::SETUP_NOTICE {
        return format!("{}\n", crate::ocr_vlm::SETUP_NOTICE);
    }
    front_matter
        .lines()
        .find_map(|line| line.strip_prefix("error: "))
        .map(|message| format!("{}\n", message.trim_matches('"')))
        .unwrap_or_else(|| "conversion failed\n".into())
}
