//! Spike: one trait, two runtimes for the doc-VLM OCR route.
//!
//! A backend turns one page image into regions (label, box, reading order,
//! text). Everything else (Markdown assembly, CER, timing) is shared.

use std::time::Duration;

use image::RgbImage;

#[cfg(feature = "candle")]
pub mod candle_backend;
#[cfg(feature = "llama")]
pub mod llama_backend;
#[cfg(feature = "llama")]
pub mod layout_ort;

/// One recognised block of a page.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Region {
    pub label: String,
    /// `[x0, y0, x1, y1]` in page pixels.
    pub bbox: [f32; 4],
    pub score: f32,
    /// Position in reading order (ascending).
    pub order: u32,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PageResult {
    pub regions: Vec<Region>,
    /// Regions that hit the token cap, timeout or repetition stop.
    pub truncated: u32,
}

impl PageResult {
    /// Reading-order text: one block per region, blank line between blocks.
    pub fn text(&self) -> String {
        let mut regions: Vec<&Region> = self.regions.iter().filter(|r| !r.text.trim().is_empty()).collect();
        regions.sort_by_key(|r| r.order);
        regions.iter().map(|r| r.text.trim()).collect::<Vec<_>>().join("\n\n")
    }
}

#[derive(Debug, Clone)]
pub struct PageOptions {
    /// Cap on generated tokens per region.
    pub max_new_tokens: usize,
    /// Wall-clock budget for the whole page (backends that cannot interrupt ignore it).
    pub page_timeout: Duration,
}

impl Default for PageOptions {
    fn default() -> Self {
        Self { max_new_tokens: 1024, page_timeout: Duration::from_secs(300) }
    }
}

/// A doc-VLM runtime: layout + region recognition for one page image.
pub trait DocOcr {
    fn name(&self) -> &'static str;
    fn recognize_page(&mut self, page: &RgbImage, options: &PageOptions) -> anyhow::Result<PageResult>;
}

/// Peak resident set size of this process in MiB.
pub fn peak_rss_mib() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let raw = usage.ru_maxrss as f64;
    // Linux reports KiB, macOS bytes.
    if cfg!(target_os = "macos") {
        raw / (1024.0 * 1024.0)
    } else {
        raw / 1024.0
    }
}

/// Character error rate: Levenshtein distance over Unicode scalar values after
/// dropping whitespace and Markdown/HTML markup, divided by the reference length.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    let a = normalise(reference);
    let b = normalise(hypothesis);
    if a.is_empty() {
        return if b.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] as f64 / a.len() as f64
}

fn normalise(text: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            c if c.is_whitespace() || matches!(c, '#' | '*' | '|' | '`' | '_' | '\u{200b}') => {}
            // Fold curly quotes and CJK punctuation variants both sides may render differently.
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{201c}' | '\u{201d}' => out.push('"'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cer_is_zero_for_equal_text_ignoring_markup() {
        assert_eq!(cer("Hello world", "# Hello  **world**"), 0.0);
    }

    #[test]
    fn cer_counts_character_edits() {
        assert!((cer("abcd", "abed") - 0.25).abs() < 1e-9);
    }
}
