//! Local layout-first OCR. The caller owns model installation and the process
//! deadline: inference must run in a killable worker, not a detached thread.

#[cfg(feature = "candle")]
pub mod candle_backend;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub label: String,
    /// `[x0, y0, x1, y1]` in image pixels, top-left origin.
    pub bbox: [f32; 4],
    /// Layout confidence, not recognition confidence.
    pub score: f32,
    pub order: u32,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageResult {
    pub regions: Vec<Region>,
    pub truncated: u32,
}

impl PageResult {
    pub fn text(&self) -> String {
        let mut regions: Vec<_> = self.regions.iter().collect();
        regions.sort_by_key(|r| r.order);
        regions
            .iter()
            .filter(|r| !r.text.trim().is_empty())
            .map(|r| r.text.trim())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

pub trait DocOcr {
    fn recognize_page(
        &mut self,
        page: &image::RgbImage,
        max_tokens: usize,
    ) -> anyhow::Result<PageResult>;
}

/// Remove a repeating suffix after three identical runs. The hard generation
/// token cap and the fork's token-run guard bound generation; this separate
/// text guard also removes repetition after task postprocessing.
pub fn stop_repetition(text: &str) -> (String, bool) {
    let chars: Vec<char> = text.chars().collect();
    for end in 24..=chars.len() {
        for width in 8..=128.min(end / 3) {
            let a = &chars[end - width..end];
            if a == &chars[end - 2 * width..end - width]
                && a == &chars[end - 3 * width..end - 2 * width]
            {
                return (chars[..end - 2 * width].iter().collect(), true);
            }
        }
    }
    (text.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repetition_is_bounded_and_unicode_safe() {
        let (text, stopped) = stop_repetition("中文一二三四五六中文一二三四五六中文一二三四五六尾");
        assert!(stopped);
        assert_eq!(text, "中文一二三四五六");
        assert!(!stop_repetition("A normal paragraph with no repeated suffix.").1);
    }
    #[test]
    fn reading_order_is_not_storage_order() {
        let region = |order, text: &str| Region {
            label: "text".into(),
            bbox: [0.; 4],
            score: 1.,
            order,
            text: text.into(),
        };
        assert_eq!(
            PageResult {
                regions: vec![region(2, "second"), region(1, "first")],
                truncated: 0
            }
            .text(),
            "first\n\nsecond"
        );
    }
}
