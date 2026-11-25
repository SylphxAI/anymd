use std::io::{Read, Write};

use crate::lean::{read_text_with_evidence, ReadRender};
use crate::schema::ReadArgs;
use crate::source_access::SourceAccessPolicy;

use super::args::Parsed;
use super::output::{error_line, CommandOutput};

pub(super) fn run(parsed: &Parsed, policy: &SourceAccessPolicy) -> Result<CommandOutput, i32> {
    if parsed.inputs.is_empty() {
        eprintln!("anymd: no input given\nRun `anymd --help` for usage.");
        return Err(2);
    }
    let mut evidence = None;
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
            timeline: None,
            source,
            node: parsed.node.clone(),
            pages: parsed.pages.clone(),
            max_tokens: parsed.max_tokens,
            cursor: parsed.cursor.clone(),
            ocr: parsed.ocr,
            transcript: Some(parsed.transcript),
            download_asr_model: Some(parsed.download_asr_model),
            images: parsed.images.clone(),
            revisions: parsed.revisions.clone(),
        };
        let render = ReadRender {
            front_matter: parsed.front_matter || several,
            unlimited: true,
        };
        match read_text_with_evidence(&args, policy, &render) {
            Ok((text, failed, kind)) => {
                if failed {
                    any_failed = true;
                    eprint!("anymd: {input}: {}", error_line(&text));
                    continue;
                }
                evidence = evidence.or(kind);
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
    Ok(CommandOutput {
        text: out,
        failed: any_failed,
        evidence,
    })
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
