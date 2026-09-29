//! Option B: oar-ocr-vl on candle (PP-DocLayoutV3 safetensors + PaddleOCR-VL-1.6).

use std::path::Path;

use anyhow::{anyhow, Result};
use image::RgbImage;
use oar_ocr_vl::utils::parse_device;
use oar_ocr_vl::{DocParser, DocParserConfig, PaddleOcrVl, PpDocLayout};

use crate::{DocOcr, PageOptions, PageResult, Region};

pub struct CandleBackend {
    vlm: PaddleOcrVl,
    layout: PpDocLayout,
}

impl CandleBackend {
    /// `vlm_dir` holds the PaddleOCR-VL-1.6 checkpoint, `layout_dir` the
    /// PP-DocLayoutV3 safetensors checkpoint. `device`: `cpu` or `metal`.
    pub fn load(vlm_dir: &Path, layout_dir: &Path, device: &str) -> Result<Self> {
        let dev = || parse_device(device).map_err(|e| anyhow!("device {device}: {e}"));
        let layout = PpDocLayout::from_dir(layout_dir, dev()?).map_err(|e| anyhow!("layout: {e}"))?;
        let vlm = PaddleOcrVl::from_dir(vlm_dir, dev()?).map_err(|e| anyhow!("vlm: {e}"))?;
        Ok(Self { vlm, layout })
    }
}

impl DocOcr for CandleBackend {
    fn name(&self) -> &'static str {
        "candle (oar-ocr-vl)"
    }

    fn recognize_page(&mut self, page: &RgbImage, options: &PageOptions) -> Result<PageResult> {
        let config = DocParserConfig { max_tokens: options.max_new_tokens, ..DocParserConfig::default() };
        let parser = DocParser::with_config(&self.vlm, config);
        let result = parser.parse(&self.layout, page.clone()).map_err(|e| anyhow!("parse: {e}"))?;
        let regions = result
            .layout_elements
            .iter()
            .enumerate()
            .map(|(i, el)| Region {
                label: el.label.clone().unwrap_or_else(|| format!("{:?}", el.element_type)),
                bbox: [el.bbox.x_min(), el.bbox.y_min(), el.bbox.x_max(), el.bbox.y_max()],
                score: el.confidence,
                order: el.order_index.unwrap_or(i as u32 + 1),
                text: el.text.clone().unwrap_or_default(),
            })
            .collect();
        Ok(PageResult { regions, truncated: 0 })
    }
}
