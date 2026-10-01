//! Local layout-first OCR. The caller owns model installation and the process
//! deadline: inference must run in a killable worker, not a detached thread.

#[cfg(feature = "candle")]
pub mod candle_backend;
pub mod hardware;

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
    pub fn markdown(&self) -> String {
        let text = self.text();
        if self.truncated == 0 {
            text
        } else {
            format!("{text}\n\n<!-- OCR generation stopped in {} regions (token limit or repetition). -->", self.truncated)
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_repeated_rows_keep_the_distinct_suffix() {
        for text in [
            "Header: abcdefghabcdefghabcdefgh; Remaining verified content.",
            "| Row | Value |\n| Row | Value |\n| Row | Value |\nDistinct verified footer.",
        ] {
            let page = PageResult {
                regions: vec![Region {
                    label: "table".into(),
                    bbox: [0.; 4],
                    score: 1.,
                    order: 1,
                    text: text.into(),
                }],
                truncated: 0,
            };
            assert_eq!(page.text(), text);
            assert_eq!(page.markdown(), text);
        }
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
