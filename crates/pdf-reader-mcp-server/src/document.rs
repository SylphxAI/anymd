//! One entry point for every input anymd reads: a local path or an http(s)
//! URL becomes an [`Opened`] document whose citable units (pages, slides,
//! sheets, chapters) convert to Markdown on demand.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anymd_formats::{ConvertError, Format};
use pdf_reader_core::markdown_layout::{self, load_document, load_document_bytes};
use pdf_reader_core::url_fetch::fetch_url;

use crate::source_access::SourceAccessPolicy;

/// Local files larger than this are refused (video is probed by path instead).
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const OCR_SCALE: f32 = 2.5;
const OCR_MAX_PIXELS: u64 = 40_000_000;
/// Pages with fewer visible characters than this count as image-only.
const SPARSE_PAGE_CHARS: usize = 24;
const CACHE_MAX_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// None = OCR image-only pages and images when `tesseract` is installed.
    pub ocr: Option<bool>,
    pub transcript: bool,
}

/// One citable unit of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// 1-based position used by `pages` selections and cursors.
    pub number: u32,
    /// Marker label: "page 3", "slide 2", "sheet Revenue", "chapter 4: …".
    pub label: String,
    pub markdown: String,
}

enum Body {
    Pdf {
        doc: Box<markdown_layout::PdfDocument>,
        bytes: Option<Arc<Vec<u8>>>,
        path: Option<PathBuf>,
        title: Option<String>,
    },
    Units(Arc<Vec<Unit>>),
}

pub struct Opened {
    /// What the caller passed (path or URL).
    pub label: String,
    pub format: &'static str,
    pub title: Option<String>,
    /// Singular unit noun for headers: "page", "slide", "sheet", "chapter", "section".
    pub unit_noun: &'static str,
    pub total: u32,
    pub metadata: Vec<(String, String)>,
    options: OpenOptions,
    body: Body,
}

/// A source spec is a URL when it starts with http:// or https://.
pub fn is_url(spec: &str) -> bool {
    let lower = spec.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn format_from_content_type(content_type: &str) -> Option<Format> {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match mime.as_str() {
        "application/pdf" | "application/x-pdf" => Format::Pdf,
        "text/html" | "application/xhtml+xml" => Format::Html,
        "text/csv" => Format::Csv,
        "text/tab-separated-values" => Format::Tsv,
        "application/epub+zip" => Format::Epub,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Format::Docx,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Format::Pptx,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel"
        | "application/vnd.oasis.opendocument.spreadsheet" => Format::Xlsx,
        "text/vtt" | "application/x-subrip" => Format::Subtitles,
        "text/plain" | "text/markdown" | "application/json" | "text/xml" | "application/xml" => {
            Format::Text
        }
        m if m.starts_with("image/") => Format::Image,
        m if m.starts_with("video/") || m.starts_with("audio/") => Format::Video,
        _ => return None,
    })
}

fn noun_for(units: &[Unit], format: Format) -> &'static str {
    let first_word = |unit: &Unit| unit.label.split_whitespace().next().unwrap_or("").to_string();
    let shared = units
        .first()
        .map(first_word)
        .filter(|word| units.iter().all(|unit| first_word(unit) == *word));
    match (shared.as_deref(), format) {
        (Some("slide"), _) => "slide",
        (Some("sheet"), _) => "sheet",
        (Some("chapter"), _) => "chapter",
        (_, Format::Pdf) => "page",
        _ => "section",
    }
}

// ---------------------------------------------------------------------------
// Converted-document cache (repeat searches and cursor reads are instant)
// ---------------------------------------------------------------------------

type CacheKey = (PathBuf, u64, u128, bool);

struct CachedDoc {
    format: &'static str,
    title: Option<String>,
    metadata: Vec<(String, String)>,
    units: Arc<Vec<Unit>>,
    bytes: usize,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<CacheKey, Arc<CachedDoc>>,
    order: VecDeque<CacheKey>,
    bytes: usize,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn cache_key(path: &Path, ocr: bool) -> Option<CacheKey> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((path.to_path_buf(), meta.len(), modified, ocr))
}

fn cache_get(key: &CacheKey) -> Option<Arc<CachedDoc>> {
    let cache = cache().lock().ok()?;
    cache.entries.get(key).cloned()
}

fn cache_put(key: CacheKey, doc: Arc<CachedDoc>) {
    let Ok(mut cache) = cache().lock() else { return };
    if doc.bytes > CACHE_MAX_BYTES / 4 {
        return;
    }
    if let Some(old) = cache.entries.insert(key.clone(), doc.clone()) {
        cache.bytes -= old.bytes;
        cache.order.retain(|k| k != &key);
    }
    cache.bytes += doc.bytes;
    cache.order.push_back(key);
    while cache.bytes > CACHE_MAX_BYTES {
        let Some(oldest) = cache.order.pop_front() else { break };
        if let Some(evicted) = cache.entries.remove(&oldest) {
            cache.bytes -= evicted.bytes;
        }
    }
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

impl Opened {
    /// Open a local path (admitted by `policy`) or an http(s) URL.
    pub fn open(spec: &str, policy: &SourceAccessPolicy, options: &OpenOptions) -> Result<Self, String> {
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
            let converted = anymd_formats::convert(
                format,
                &head,
                &anymd_formats::Options {
                    path: Some(path.clone()),
                    transcript: options.transcript,
                    ..Default::default()
                },
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
        let key = cache_key(&path, ocr_on);
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
                    title: opened.title.clone(),
                    metadata: opened.metadata.clone(),
                    units: units.clone(),
                    bytes: units.iter().map(|u| u.markdown.len()).sum(),
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
            return Ok(Self::from_pdf(url, doc, Some(Arc::new(fetched.bytes)), None, options));
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
            body: Body::Pdf {
                doc: Box::new(doc),
                bytes,
                path,
                title,
            },
        }
    }

    fn from_converted(
        label: &str,
        format: Format,
        converted: anymd_formats::Converted,
        options: &OpenOptions,
    ) -> Self {
        let units: Vec<Unit> = converted
            .sections
            .into_iter()
            .enumerate()
            .map(|(index, section)| Unit {
                number: index as u32 + 1,
                label: section.label,
                markdown: section.markdown,
            })
            .collect();
        let noun = noun_for(&units, format);
        Self {
            label: label.to_string(),
            format: format.name(),
            title: converted.title,
            unit_noun: noun,
            total: units.len() as u32,
            metadata: converted.metadata,
            options: options.clone(),
            body: Body::Units(Arc::new(units)),
        }
    }

    fn from_cached(label: &str, cached: &CachedDoc, options: &OpenOptions) -> Self {
        let format = anymd_formats_format(cached.format);
        Self {
            label: label.to_string(),
            format: cached.format,
            title: cached.title.clone(),
            unit_noun: noun_for(&cached.units, format),
            total: cached.units.len() as u32,
            metadata: cached.metadata.clone(),
            options: options.clone(),
            body: Body::Units(cached.units.clone()),
        }
    }

    /// True when every unit converts in one pass (not a lazily paged PDF).
    pub fn is_paged(&self) -> bool {
        matches!(self.body, Body::Pdf { .. })
    }

    /// Bookmarks (PDF only): (depth, title, page).
    pub fn outline(&self) -> Vec<(usize, String, Option<u32>)> {
        match &self.body {
            Body::Pdf { doc, .. } => markdown_layout::outline(doc),
            Body::Units(_) => Vec::new(),
        }
    }

    /// Convert the requested units (1-based numbers, ascending).
    pub fn units(&self, numbers: &[u32]) -> Result<Vec<Unit>, String> {
        match &self.body {
            Body::Units(units) => Ok(numbers
                .iter()
                .filter_map(|n| units.get(*n as usize - 1).cloned())
                .collect()),
            Body::Pdf { doc, bytes, path, title } => {
                let converted = markdown_layout::pdf_to_markdown(doc, Some(numbers))
                    .map_err(|error| error.message)?;
                let _ = title;
                let mut units: Vec<Unit> = converted
                    .pages
                    .into_iter()
                    .map(|page| Unit {
                        number: page.number,
                        label: format!("page {}", page.number),
                        markdown: page.markdown,
                    })
                    .collect();
                self.ocr_sparse_pages(&mut units, bytes.as_deref(), path.as_deref());
                Ok(units)
            }
        }
    }

    /// Title from page 1 when the PDF has no usable /Title.
    pub fn title_from_units(&mut self, units: &[Unit]) {
        if self.title.is_some() {
            return;
        }
        if let Some(first) = units.iter().find(|unit| unit.number == 1) {
            self.title = first
                .markdown
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .map(|title| title.trim().to_string());
        }
    }

    /// Every unit (PDFs convert in parallel chunks). Cached for local files.
    pub fn all_units(&self) -> Result<Arc<Vec<Unit>>, String> {
        match &self.body {
            Body::Units(units) => Ok(units.clone()),
            Body::Pdf { path, .. } => {
                let ocr_on = self.options.ocr != Some(false) && anymd_formats::image::ocr_available();
                let key = path.as_deref().and_then(|p| cache_key(p, ocr_on));
                if let Some(cached) = key.as_ref().and_then(cache_get) {
                    return Ok(cached.units.clone());
                }
                let numbers: Vec<u32> = (1..=self.total).collect();
                let units = Arc::new(self.units(&numbers)?);
                if let Some(key) = key {
                    cache_put(
                        key,
                        Arc::new(CachedDoc {
                            format: "pdf",
                            title: self.title.clone(),
                            metadata: Vec::new(),
                            units: units.clone(),
                            bytes: units.iter().map(|u| u.markdown.len()).sum(),
                        }),
                    );
                }
                Ok(units)
            }
        }
    }

    fn ocr_sparse_pages(&self, units: &mut [Unit], bytes: Option<&Vec<u8>>, path: Option<&Path>) {
        let wanted = match self.options.ocr {
            Some(false) => return,
            Some(true) => true,
            None => false,
        };
        let sparse: Vec<usize> = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| visible_chars(&unit.markdown) < SPARSE_PAGE_CHARS)
            .map(|(index, _)| index)
            .collect();
        if sparse.is_empty() {
            return;
        }
        if !anymd_formats::image::ocr_available() {
            if wanted {
                for index in sparse {
                    units[index].markdown.push_str(
                        "\n\n<!-- OCR needs `tesseract` installed (apt install tesseract-ocr / brew install tesseract). -->",
                    );
                }
            }
            return;
        }
        let pdf_bytes = match (bytes, path) {
            (Some(bytes), _) => bytes.clone(),
            (None, Some(path)) => match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(_) => return,
            },
            (None, None) => return,
        };
        let Ok(renderer) = pdf_reader_core::render::RenderDocument::new(pdf_bytes) else {
            return;
        };
        for index in sparse {
            let page = units[index].number as usize;
            let text = renderer
                .render_page(page, OCR_SCALE, OCR_MAX_PIXELS, 256 * 1024 * 1024)
                .map_err(|error| error.message)
                .and_then(|rendered| anymd_formats::image::ocr_text(&rendered.png, ".png"));
            match text {
                Ok(text) if !text.trim().is_empty() => {
                    let existing = units[index].markdown.trim().to_string();
                    units[index].markdown = if existing.is_empty() {
                        format!("<!-- OCR text -->\n\n{text}")
                    } else {
                        format!("{existing}\n\n<!-- OCR text -->\n\n{text}")
                    };
                }
                Ok(_) => {}
                Err(error) => {
                    units[index]
                        .markdown
                        .push_str(&format!("\n\n<!-- OCR failed: {error} -->"));
                }
            }
        }
    }
}

fn anymd_formats_format(name: &str) -> Format {
    match name {
        "pdf" => Format::Pdf,
        "docx" => Format::Docx,
        "pptx" => Format::Pptx,
        "xlsx" => Format::Xlsx,
        "csv" => Format::Csv,
        "tsv" => Format::Tsv,
        "epub" => Format::Epub,
        "html" => Format::Html,
        "image" => Format::Image,
        "video" => Format::Video,
        "subtitles" => Format::Subtitles,
        _ => Format::Text,
    }
}

fn visible_chars(markdown: &str) -> usize {
    markdown
        .lines()
        .filter(|line| !line.trim_start().starts_with("<!--"))
        .flat_map(str::chars)
        .filter(|c| c.is_alphanumeric())
        .count()
}

fn read_head(path: &Path, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut head = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        let read = file.read(&mut head[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    head.truncate(filled);
    Ok(head)
}

fn convert_other(
    format: Format,
    bytes: &[u8],
    base_url: Option<String>,
    path: Option<PathBuf>,
    options: &OpenOptions,
) -> Result<anymd_formats::Converted, ConvertError> {
    let ocr = match options.ocr {
        Some(value) => value,
        None => format == Format::Image && anymd_formats::image::ocr_available(),
    };
    anymd_formats::convert(
        format,
        bytes,
        &anymd_formats::Options {
            base_url,
            ocr,
            transcript: options.transcript,
            path,
        },
    )
}

/// File extensions a directory walk picks up for search.
pub fn searchable_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf" | "docx" | "pptx" | "xlsx" | "xls" | "xlsm" | "ods" | "csv" | "tsv" | "epub"
            | "html" | "htm" | "xhtml" | "md" | "markdown" | "txt" | "rst" | "srt" | "vtt"
    )
}

/// Readable file extensions (search set plus media).
pub fn readable_extension(path: &Path) -> bool {
    searchable_extension(path)
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "tif" | "tiff" | "bmp" | "mp4"
                        | "mov" | "mkv" | "webm" | "mp3" | "wav" | "m4a" | "flac" | "ogg"
                )
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_map_to_formats() {
        assert_eq!(format_from_content_type("application/pdf"), Some(Format::Pdf));
        assert_eq!(format_from_content_type("text/html; charset=utf-8"), Some(Format::Html));
        assert_eq!(format_from_content_type("image/png"), Some(Format::Image));
        assert_eq!(format_from_content_type("application/octet-stream"), None);
    }

    #[test]
    fn nouns_follow_section_labels() {
        let unit = |label: &str| Unit { number: 1, label: label.into(), markdown: String::new() };
        assert_eq!(noun_for(&[unit("slide 1"), unit("slide 2")], Format::Pptx), "slide");
        assert_eq!(noun_for(&[unit("sheet A"), unit("sheet B")], Format::Xlsx), "sheet");
        assert_eq!(noun_for(&[unit("document")], Format::Docx), "section");
    }

    #[test]
    fn opens_csv_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        let opened = Opened::open(
            path.to_str().unwrap(),
            &SourceAccessPolicy::unrestricted(),
            &OpenOptions::default(),
        )
        .unwrap();
        assert_eq!(opened.format, "csv");
        let units = opened.units(&[1]).unwrap();
        assert!(units[0].markdown.contains("| a | b |"), "{}", units[0].markdown);
    }
}
