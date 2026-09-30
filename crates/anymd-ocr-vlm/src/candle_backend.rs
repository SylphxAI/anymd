use crate::{stop_repetition, DocOcr, PageResult, Region};
use anyhow::{anyhow, Result};
use image::RgbImage;
use oar_ocr_vl::utils::parse_device;
use oar_ocr_vl::{DocParser, DocParserConfig, PaddleOcrVl, PpDocLayout};
use std::path::Path;

pub struct CandleBackend {
    vlm: PaddleOcrVl,
    layout: PpDocLayout,
}

impl CandleBackend {
    pub fn load(vlm_dir: &Path, layout_dir: &Path, device: &str) -> Result<Self> {
        let dev = || parse_device(device).map_err(|e| anyhow!("device {device}: {e}"));
        let layout =
            PpDocLayout::from_dir(layout_dir, dev()?).map_err(|e| anyhow!("layout: {e}"))?;
        let vlm = PaddleOcrVl::from_dir(vlm_dir, dev()?).map_err(|e| anyhow!("vlm: {e}"))?;
        Ok(Self { vlm, layout })
    }

    pub fn metal_available() -> bool {
        cfg!(target_os = "macos") && parse_device("metal").is_ok()
    }
}

impl DocOcr for CandleBackend {
    fn recognize_page(&mut self, page: &RgbImage, max_tokens: usize) -> Result<PageResult> {
        anyhow::ensure!(
            (1..=8192).contains(&max_tokens),
            "OCR token cap must be 1..8192"
        );
        let config = DocParserConfig {
            max_tokens,
            skip_auxiliary_regions: false,
            markdown_pretty: false,
            ..DocParserConfig::default()
        };
        let result = DocParser::with_config(&self.vlm, config)
            .parse(&self.layout, page.clone())
            .map_err(|e| anyhow!("parse: {e}"))?;
        let mut truncated = 0;
        let regions = result
            .layout_elements
            .iter()
            .enumerate()
            .map(|(i, el)| {
                let (text, stopped) = stop_repetition(el.text.as_deref().unwrap_or_default());
                truncated += u32::from(stopped);
                Region {
                    label: el
                        .label
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", el.element_type)),
                    bbox: [
                        el.bbox.x_min(),
                        el.bbox.y_min(),
                        el.bbox.x_max(),
                        el.bbox.y_max(),
                    ],
                    score: el.confidence,
                    order: el.order_index.unwrap_or(i as u32 + 1),
                    text,
                }
            })
            .collect();
        Ok(PageResult { regions, truncated })
    }
}
