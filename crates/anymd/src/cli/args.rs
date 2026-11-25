pub(super) struct Parsed {
    pub(super) node: Option<String>,
    pub(super) format: Option<String>,
    pub(super) inputs: Vec<String>,
    pub(super) pages: Option<String>,
    pub(super) output: Option<String>,
    pub(super) max_tokens: Option<u32>,
    pub(super) cursor: Option<String>,
    pub(super) ocr: Option<crate::ocr_vlm::OcrSelection>,
    pub(super) transcript: bool,
    pub(super) download_asr_model: bool,
    pub(super) images: Option<String>,
    pub(super) revisions: Option<String>,
    pub(super) front_matter: bool,
    pub(super) mode: Option<String>,
    pub(super) glob: Option<String>,
    pub(super) max: Option<u32>,
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
}

pub(super) fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let mut parsed = Parsed {
        node: None,
        format: None,
        inputs: Vec::new(),
        pages: None,
        output: None,
        max_tokens: None,
        cursor: None,
        ocr: None,
        transcript: false,
        download_asr_model: false,
        images: None,
        revisions: None,
        front_matter: false,
        mode: None,
        glob: None,
        max: None,
        case_sensitive: false,
        whole_word: false,
    };
    let mut iter = arguments.iter().peekable();
    while let Some(argument) = iter.next() {
        if argument == "--" {
            parsed.inputs.extend(iter.cloned());
            break;
        }
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
            "--node" => parsed.node = Some(value(flag)?),
            "--format" => parsed.format = Some(value(flag)?),
            "--json" => parsed.format = Some("json".into()),
            "--cursor" => parsed.cursor = Some(value(flag)?),
            "--ocr" => {
                let is_engine = iter
                    .peek()
                    .is_some_and(|v| matches!(v.as_str(), "auto" | "vlm" | "tesseract"));
                let named = inline.or_else(|| {
                    if is_engine {
                        iter.next().cloned()
                    } else {
                        None
                    }
                });
                parsed.ocr = Some(match named {
                    Some(v) => crate::ocr_vlm::OcrSelection::parse(&v)?,
                    None => crate::ocr_vlm::OcrSelection::Enabled(true),
                });
            }
            "--no-ocr" => parsed.ocr = Some(crate::ocr_vlm::OcrSelection::Enabled(false)),
            "--transcript" => parsed.transcript = true,
            "--download-asr-model" | "--download-whisper-model" => {
                if inline.is_some() {
                    return Err(format!(
                        "{flag} is a presence switch and does not accept a value"
                    ));
                }
                parsed.download_asr_model = true;
            }
            "--images" => {
                let mode = value(flag)?;
                if mode != "refs" && mode != "none" {
                    return Err("--images needs refs or none".to_string());
                }
                parsed.images = Some(mode);
            }
            "--revisions" => {
                let mode = value(flag)?;
                if anymd_formats::Revisions::parse(&mode).is_none() {
                    return Err("--revisions needs markup, accept or reject".to_string());
                }
                parsed.revisions = Some(mode);
            }
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
