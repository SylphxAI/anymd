//! Command-line mode: `anymd <file|url|dir>...` prints Markdown to stdout,
//! `anymd search <query> [paths...]` prints hits. With no arguments and a
//! piped stdin the binary runs the MCP server instead (see main.rs).

use std::io::{IsTerminal, Read, Write};

use crate::lean::{read_text, result_text, search, ReadRender};
use crate::schema::{ReadArgs, SearchArgs};
use crate::source_access::SourceAccessPolicy;

pub const USAGE: &str = "\
anymd — any file → clean Markdown for AI agents

Usage:
  anymd <file|url|dir>... [options]   Convert to Markdown on stdout
  anymd - [options]                   Convert stdin
  anymd search <query> [path|url...]  Search files and directories (default: .)
  anymd mcp [--allow-dir=<path>]...   Run the MCP server on stdio
                                      (also the default when stdin is piped and no file is given)
  anymd doctor                        Print version and optional tool availability

Read options:
  -p, --pages <spec>       Pages, slides, sheets, or chapters, e.g. 1-5,8
  -o, --output <file>      Write to a file instead of stdout
      --max-tokens <n>     Stop at a token budget and print a cursor
      --cursor <cursor>    Continue from a cursor
      --ocr / --no-ocr     Force or disable OCR (default: automatic when tesseract is installed)
      --transcript         Transcribe audio/video with a local whisper.cpp
      --download-whisper-model
                           With --transcript (implied): download the ggml model on first use
                           (base.en, ~148 MB; ANYMD_WHISPER_MODEL_SIZE=tiny|base|small[.en])
      --front-matter       Print the source/title/pages header (always on for several inputs)

Search options:
      --mode <m>           auto (default), literal, or ranked (BM25)
      --glob <glob>        Only files matching the glob, e.g. '*.pdf'
      --max <n>            Maximum hits (default 20)
  -i, --case-sensitive     Match case
  -w, --whole-word         Match whole words

  -h, --help               Show this help
  -V, --version            Show the version

Formats: PDF, DOCX, PPTX, XLSX/XLS/ODS, CSV/TSV, EPUB, HTML and web URLs, Markdown/text,
images (metadata + OCR), audio/video (metadata, chapters, subtitles), SRT/VTT.";

/// What main should do with the process arguments.
pub enum Mode {
    Mcp,
    Doctor,
    Cli(Vec<String>),
}

/// Decide between MCP server and CLI. MCP configs pass no file arguments
/// (optionally `--allow-dir=...`) and pipe stdin.
pub fn mode(arguments: &[String]) -> Mode {
    match arguments.first().map(String::as_str) {
        Some("mcp") | Some("serve") => return Mode::Mcp,
        Some("doctor") => return Mode::Doctor,
        _ => {}
    }
    let only_server_flags = arguments
        .iter()
        .all(|argument| argument.starts_with("--allow-dir"));
    if only_server_flags && (!arguments.is_empty() || !std::io::stdin().is_terminal()) {
        return Mode::Mcp;
    }
    Mode::Cli(arguments.to_vec())
}

struct Parsed {
    inputs: Vec<String>,
    pages: Option<String>,
    output: Option<String>,
    max_tokens: Option<u32>,
    cursor: Option<String>,
    ocr: Option<bool>,
    transcript: bool,
    download_whisper_model: bool,
    front_matter: bool,
    mode: Option<String>,
    glob: Option<String>,
    max: Option<u32>,
    case_sensitive: bool,
    whole_word: bool,
}

fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let mut parsed = Parsed {
        inputs: Vec::new(),
        pages: None,
        output: None,
        max_tokens: None,
        cursor: None,
        ocr: None,
        transcript: false,
        download_whisper_model: false,
        front_matter: false,
        mode: None,
        glob: None,
        max: None,
        case_sensitive: false,
        whole_word: false,
    };
    let mut iter = arguments.iter().peekable();
    while let Some(argument) = iter.next() {
        let (flag, inline) = match argument.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
            _ => (argument.as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| iter.next().cloned())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag {
            "-p" | "--pages" => parsed.pages = Some(value(flag)?),
            "-o" | "--output" => parsed.output = Some(value(flag)?),
            "--max-tokens" => {
                parsed.max_tokens = Some(
                    value(flag)?
                        .parse()
                        .map_err(|_| "--max-tokens needs a number".to_string())?,
                )
            }
            "--cursor" => parsed.cursor = Some(value(flag)?),
            "--ocr" => parsed.ocr = Some(true),
            "--no-ocr" => parsed.ocr = Some(false),
            "--transcript" => parsed.transcript = true,
            "--download-whisper-model" => parsed.download_whisper_model = true,
            "--front-matter" => parsed.front_matter = true,
            "--mode" => parsed.mode = Some(value(flag)?),
            "--glob" => parsed.glob = Some(value(flag)?),
            "--max" => {
                parsed.max = Some(
                    value(flag)?
                        .parse()
                        .map_err(|_| "--max needs a number".to_string())?,
                )
            }
            "-i" | "--case-sensitive" => parsed.case_sensitive = true,
            "-w" | "--whole-word" => parsed.whole_word = true,
            flag if flag.starts_with("--allow-dir") => {}
            "-" => parsed.inputs.push("-".into()),
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(format!("unknown option {flag}"));
            }
            _ => parsed.inputs.push(argument.clone()),
        }
    }
    Ok(parsed)
}

fn stdin_to_temp() -> Result<tempfile::NamedTempFile, String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("reading stdin: {error}"))?;
    let format = anymd_formats::detect(None, &bytes).ok_or("stdin: unrecognized content")?;
    let suffix = match format {
        anymd_formats::Format::Pdf => ".pdf",
        anymd_formats::Format::Docx => ".docx",
        anymd_formats::Format::Pptx => ".pptx",
        anymd_formats::Format::Xlsx => ".xlsx",
        anymd_formats::Format::Csv => ".csv",
        anymd_formats::Format::Tsv => ".tsv",
        anymd_formats::Format::Epub => ".epub",
        anymd_formats::Format::Html => ".html",
        anymd_formats::Format::Image => ".img",
        anymd_formats::Format::Video => ".media",
        anymd_formats::Format::Subtitles => ".srt",
        anymd_formats::Format::Text => ".txt",
    };
    let mut file = tempfile::Builder::new()
        .prefix("anymd-stdin-")
        .suffix(suffix)
        .tempfile()
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    Ok(file)
}

/// Run the CLI; returns the process exit code.
pub fn run(arguments: Vec<String>, policy: &SourceAccessPolicy) -> i32 {
    if arguments.iter().any(|a| a == "-h" || a == "--help") || arguments.is_empty() {
        println!("{USAGE}");
        return if arguments.is_empty() { 2 } else { 0 };
    }
    if arguments.iter().any(|a| a == "-V" || a == "--version") {
        println!("anymd {}", crate::SERVER_VERSION);
        return 0;
    }
    let is_search = arguments.first().map(String::as_str) == Some("search");
    let parsed = match parse(if is_search {
        &arguments[1..]
    } else {
        &arguments[..]
    }) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("anymd: {message}\nRun `anymd --help` for usage.");
            return 2;
        }
    };
    let (text, failed) = if is_search {
        let Some((query, sources)) = parsed.inputs.split_first() else {
            eprintln!("anymd: search needs a query");
            return 2;
        };
        let args = SearchArgs {
            query: query.clone(),
            sources: sources.to_vec(),
            mode: parsed.mode.clone(),
            glob: parsed.glob.clone(),
            case_sensitive: Some(parsed.case_sensitive),
            whole_word: Some(parsed.whole_word),
            max_results: parsed.max,
            context_chars: None,
        };
        match search(&args, policy) {
            Ok(result) => (result_text(&result), result.is_error == Some(true)),
            Err(error) => {
                eprintln!("anymd: {}", error.message);
                return 2;
            }
        }
    } else {
        if parsed.inputs.is_empty() {
            eprintln!("anymd: no input given\nRun `anymd --help` for usage.");
            return 2;
        }
        let several = parsed.inputs.len() > 1;
        let mut out = String::new();
        let mut any_failed = false;
        let mut _stdin_file = None;
        for input in &parsed.inputs {
            let source = if input == "-" {
                match stdin_to_temp() {
                    Ok(file) => {
                        let path = file.path().display().to_string();
                        _stdin_file = Some(file);
                        path
                    }
                    Err(message) => {
                        eprintln!("anymd: {message}");
                        any_failed = true;
                        continue;
                    }
                }
            } else {
                input.clone()
            };
            let args = ReadArgs {
                source,
                pages: parsed.pages.clone(),
                max_tokens: parsed.max_tokens,
                cursor: parsed.cursor.clone(),
                ocr: parsed.ocr,
                transcript: Some(parsed.transcript),
                download_whisper_model: Some(parsed.download_whisper_model),
            };
            let render = ReadRender {
                front_matter: parsed.front_matter || several,
                unlimited: true,
            };
            match read_text(&args, policy, &render) {
                Ok((text, failed)) => {
                    if failed {
                        any_failed = true;
                        eprint!("anymd: {input}: {}", error_line(&text));
                        continue;
                    }
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&text);
                }
                Err(message) => {
                    any_failed = true;
                    eprintln!("anymd: {input}: {message}");
                }
            }
        }
        (out, any_failed)
    };
    let written = match &parsed.output {
        Some(path) => {
            std::fs::write(path, text.as_bytes()).map_err(|error| format!("{path}: {error}"))
        }
        None => std::io::stdout()
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string()),
    };
    if let Err(message) = written {
        eprintln!("anymd: {message}");
        return 1;
    }
    i32::from(failed)
}

fn error_line(front_matter: &str) -> String {
    front_matter
        .lines()
        .find_map(|line| line.strip_prefix("error: "))
        .map(|message| format!("{}\n", message.trim_matches('"')))
        .unwrap_or_else(|| "conversion failed\n".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn server_flags_and_subcommands_select_mcp() {
        assert!(matches!(mode(&args(&["mcp"])), Mode::Mcp));
        assert!(matches!(mode(&args(&["--allow-dir=/tmp"])), Mode::Mcp));
        assert!(matches!(mode(&args(&["doctor"])), Mode::Doctor));
        assert!(matches!(mode(&args(&["paper.pdf"])), Mode::Cli(_)));
    }

    #[test]
    fn parses_read_and_search_options() {
        let parsed = parse(&args(&[
            "a.pdf",
            "-p",
            "1-3",
            "--max-tokens=500",
            "--no-ocr",
        ]))
        .unwrap();
        assert_eq!(parsed.inputs, ["a.pdf"]);
        assert_eq!(parsed.pages.as_deref(), Some("1-3"));
        assert_eq!(parsed.max_tokens, Some(500));
        assert_eq!(parsed.ocr, Some(false));
        let parsed = parse(&args(&["attention", "docs", "--mode", "ranked", "-w"])).unwrap();
        assert_eq!(parsed.inputs, ["attention", "docs"]);
        assert_eq!(parsed.mode.as_deref(), Some("ranked"));
        assert!(parsed.whole_word);
        assert!(parse(&args(&["--bogus"])).is_err());
        let parsed = parse(&args(&["talk.mp4", "--download-whisper-model"])).unwrap();
        assert!(parsed.download_whisper_model);
        assert_eq!(parsed.inputs, ["talk.mp4"]);
    }

    #[test]
    fn converts_a_file_to_stdout_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        let (text, failed) = read_text(
            &ReadArgs {
                source: path.display().to_string(),
                pages: None,
                max_tokens: None,
                cursor: None,
                ocr: None,
                transcript: None,
                download_whisper_model: None,
            },
            &SourceAccessPolicy::unrestricted(),
            &ReadRender {
                front_matter: false,
                unlimited: true,
            },
        )
        .unwrap();
        assert!(!failed);
        assert!(text.starts_with("|a|b|"), "{text}");
    }
}
