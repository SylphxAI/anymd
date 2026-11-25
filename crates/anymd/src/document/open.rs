use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anymd_core::document::{format_from_content_type, units_from_sections};
use anymd_core::markdown_layout::{self, load_document, load_document_bytes};
use anymd_core::outline::noun_for_units;
use anymd_core::url_fetch::fetch_url;
use anymd_formats::Format;

use crate::source_access::SourceAccessPolicy;

use super::cache::{cache_get, cache_key, cache_put, referenced_images, CachedDoc};
use super::{convert_other, is_url, read_head, Body, OpenOptions, Opened, Unit};

/// Local files larger than this are refused (video is probed by path instead).
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

impl Opened {
    pub(crate) fn from_video(label: &str, evidence: &crate::video_evidence::VideoEvidence) -> Self {
        let units = crate::video_evidence::sections(evidence)
            .into_iter()
            .enumerate()
            .map(|(index, section)| Unit {
                number: index as u32 + 1,
                label: format!("section {}", index + 1),
                markdown: format!("# {}\n\n{}", section.label, section.markdown),
            })
            .collect::<Vec<_>>();
        Self {
            label: label.into(),
            format: "video",
            title: None,
            unit_noun: "section",
            total: units.len() as u32,
            metadata: vec![(
                "source_sha256".into(),
                evidence.timeline.source_sha256.clone(),
            )],
            options: OpenOptions::default(),
            body: Body::Units(Arc::new(units)),
            native_outline: vec![],
            ocr_request: Mutex::new(None),
            ocr_ran: Mutex::new(Vec::new()),
        }
    }

    /// Open a local path (admitted by `policy`) or an http(s) URL.
    pub fn open(
        spec: &str,
        policy: &SourceAccessPolicy,
        options: &OpenOptions,
    ) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("source is empty".into());
        }
        if is_url(spec) {
            return Self::open_url(spec, options);
        }
        let admitted = policy.admit_path(spec)?;
        let path = PathBuf::from(&admitted);
        let meta = std::fs::metadata(&path).map_err(|error| format!("{spec}: {error}"))?;
        if meta.is_dir() {
            return Err(format!("{spec} is a directory"));
        }
        let head = read_head(&path, 8192).map_err(|error| format!("{spec}: {error}"))?;
        let format = anymd_formats::detect(Some(&path), &head)
            .ok_or_else(|| format!("{spec}: unrecognized file type"))?;
        if format == Format::Video {
            let converted = anymd_formats::convert_with_source_access(
                format,
                &head,
                &anymd_formats::Options {
                    path: Some(path.clone()),
                    transcript: options.transcript,
                    download_asr_model: options.download_asr_model,
                    ..Default::default()
                },
                policy,
            )
            .map_err(|error| format!("{spec}: {error}"))?;
            return Ok(Self::from_converted(spec, format, converted, options));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(format!(
                "{spec}: file is {} MB; the limit is {} MB",
                meta.len() / 1_048_576,
                MAX_FILE_BYTES / 1_048_576
            ));
        }
        let ocr_on = options.ocr != Some(false) && anymd_formats::image::ocr_available();
        let key = cache_key(&path, ocr_on, options);
        if format != Format::Pdf {
            if let Some(cached) = key.as_ref().and_then(cache_get) {
                return Ok(Self::from_cached(spec, &cached, options));
            }
        }
        if format == Format::Pdf {
            let doc = load_document(&path).map_err(|error| format!("{spec}: {}", error.message))?;
            return Ok(Self::from_pdf(spec, doc, None, Some(path), options));
        }
        let bytes = std::fs::read(&path).map_err(|error| format!("{spec}: {error}"))?;
        let converted = convert_other(format, &bytes, None, Some(path.clone()), options)
            .map_err(|error| format!("{spec}: {error}"))?;
        let opened = Self::from_converted(spec, format, converted, options);
        if let (Some(key), Body::Units(units)) = (key, &opened.body) {
            cache_put(
                key,
                Arc::new(CachedDoc {
                    format: opened.format,
                    native_outline: opened.native_outline.clone(),
                    title: opened.title.clone(),
                    metadata: opened.metadata.clone(),
                    units: units.clone(),
                    bytes: units.iter().map(|u| u.markdown.len()).sum(),
                    images: referenced_images(units, options.images.as_ref()),
                }),
            );
        }
        Ok(opened)
    }

    fn open_url(url: &str, options: &OpenOptions) -> Result<Self, String> {
        let fetched = fetch_url(url)?;
        let path_hint = url::Url::parse(&fetched.final_url)
            .ok()
            .map(|u| PathBuf::from(u.path()));
        let format = if fetched.bytes.starts_with(b"%PDF-") {
            Format::Pdf
        } else {
            fetched
                .content_type
                .as_deref()
                .and_then(format_from_content_type)
                .filter(|format| *format != Format::Text)
                .or_else(|| anymd_formats::detect(path_hint.as_deref(), &fetched.bytes))
                .ok_or_else(|| format!("{url}: unrecognized content type"))?
        };
        if format == Format::Pdf {
            let doc = load_document_bytes(&fetched.bytes)
                .map_err(|error| format!("{url}: {}", error.message))?;
            return Ok(Self::from_pdf(
                url,
                doc,
                Some(Arc::new(fetched.bytes)),
                None,
                options,
            ));
        }
        let converted = convert_other(
            format,
            &fetched.bytes,
            Some(fetched.final_url.clone()),
            None,
            options,
        )
        .map_err(|error| format!("{url}: {error}"))?;
        Ok(Self::from_converted(url, format, converted, options))
    }

    fn from_pdf(
        label: &str,
        doc: markdown_layout::PdfDocument,
        bytes: Option<Arc<Vec<u8>>>,
        path: Option<PathBuf>,
        options: &OpenOptions,
    ) -> Self {
        let total = markdown_layout::page_count(&doc);
        let title = markdown_layout::info_title(&doc);
        Self {
            label: label.to_string(),
            format: "pdf",
            title: title.clone(),
            unit_noun: "page",
            total,
            metadata: Vec::new(),
            options: options.clone(),
            native_outline: Vec::new(),
            ocr_request: Mutex::new(None),
            ocr_ran: Mutex::new(Vec::new()),
            body: Body::Pdf {
                doc: Box::new(doc),
                bytes,
                path,
                title,
                repeated: OnceLock::new(),
            },
        }
    }

    fn from_converted(
        label: &str,
        format: Format,
        converted: anymd_formats::Converted,
        options: &OpenOptions,
    ) -> Self {
        let units = units_from_sections(converted.sections);
        let noun = noun_for_units(&units, format == Format::Pdf);
        Self {
            label: label.to_string(),
            format: format.name(),
            title: converted.title,
            unit_noun: noun,
            total: units.len() as u32,
            metadata: converted.metadata,
            options: options.clone(),
            native_outline: converted.outline,
            ocr_request: Mutex::new(None),
            ocr_ran: Mutex::new(Vec::new()),
            body: Body::Units(Arc::new(units)),
        }
    }

    fn from_cached(label: &str, cached: &CachedDoc, options: &OpenOptions) -> Self {
        Self {
            label: label.to_string(),
            format: cached.format,
            title: cached.title.clone(),
            unit_noun: noun_for_units(&cached.units, cached.format == "pdf"),
            total: cached.units.len() as u32,
            metadata: cached.metadata.clone(),
            options: options.clone(),
            native_outline: cached.native_outline.clone(),
            ocr_request: Mutex::new(None),
            ocr_ran: Mutex::new(Vec::new()),
            body: Body::Units(cached.units.clone()),
        }
    }
}
