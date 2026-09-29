//! PP-DocLayoutV3 through ONNX Runtime. The exported graph does its own box
//! decoding and reading order: output 0 is `[300, 7]` rows of
//! `[label, score, x0, y0, x1, y1, read_order]` in original-image pixels.

use std::path::Path;

use anyhow::{anyhow, Result};
use image::{imageops::FilterType, RgbImage};
use ort::session::Session;
use ort::value::Tensor;

use crate::Region;

const SIZE: u32 = 800;

pub const LABELS: [&str; 25] = [
    "abstract", "algorithm", "aside_text", "chart", "content", "display_formula", "doc_title",
    "figure_title", "footer", "footer_image", "footnote", "formula_number", "header",
    "header_image", "image", "inline_formula", "number", "paragraph_title", "reference",
    "reference_content", "seal", "table", "text", "vertical_text", "vision_footnote",
];

pub struct LayoutOrt {
    session: Session,
    imagenet: bool,
    threshold: f32,
}

impl LayoutOrt {
    pub fn load(onnx: &Path, threads: usize) -> Result<Self> {
        let session = Session::builder()
            .map_err(|e| anyhow!("ort builder: {e}"))?
            .with_intra_threads(threads)
            .map_err(|e| anyhow!("ort threads: {e}"))?
            .commit_from_file(onnx)
            .map_err(|e| anyhow!("ort load {}: {e}", onnx.display()))?;
        // The model card's Python sample normalises with ImageNet statistics, the Paddle
        // inference.yml does not. Both give the same boxes on our pages; default to the yml.
        let imagenet = std::env::var("ANYMD_LAYOUT_NORM").is_ok_and(|v| v == "imagenet");
        Ok(Self { session, imagenet, threshold: 0.5 })
    }

    /// Regions in reading order.
    pub fn detect(&mut self, page: &RgbImage) -> Result<Vec<Region>> {
        let (w, h) = page.dimensions();
        let resized = image::imageops::resize(page, SIZE, SIZE, FilterType::Triangle);
        let plane = (SIZE * SIZE) as usize;
        let mut chw = vec![0f32; 3 * plane];
        let (mean, std) = if self.imagenet {
            ([0.485f32, 0.456, 0.406], [0.229f32, 0.224, 0.225])
        } else {
            ([0.0; 3], [1.0; 3])
        };
        for (i, px) in resized.pixels().enumerate() {
            for c in 0..3 {
                chw[c * plane + i] = (px[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
        let names: Vec<String> = self.session.inputs().iter().map(|i| i.name().to_string()).collect();
        if names.len() < 3 {
            return Err(anyhow!("layout model has {} inputs, expected 3", names.len()));
        }
        let im_shape = Tensor::from_array((vec![1i64, 2], vec![SIZE as f32, SIZE as f32]))?;
        let image = Tensor::from_array((vec![1i64, 3, SIZE as i64, SIZE as i64], chw))?;
        let scale = Tensor::from_array((vec![1i64, 2], vec![SIZE as f32 / h as f32, SIZE as f32 / w as f32]))?;
        let outputs = self.session.run(ort::inputs![
            names[0].as_str() => im_shape,
            names[1].as_str() => image,
            names[2].as_str() => scale
        ])?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        let cols = shape.last().copied().unwrap_or(7) as usize;
        if cols < 7 {
            return Err(anyhow!("layout output has {cols} columns, expected 7"));
        }
        let mut regions: Vec<(f32, Region)> = data
            .chunks_exact(cols)
            .filter(|r| r[1] >= self.threshold)
            .map(|r| {
                let label = LABELS.get(r[0] as usize).copied().unwrap_or("text").to_string();
                let bbox = [
                    r[2].clamp(0.0, w as f32),
                    r[3].clamp(0.0, h as f32),
                    r[4].clamp(0.0, w as f32),
                    r[5].clamp(0.0, h as f32),
                ];
                (r[6], Region { label, bbox, score: r[1], order: 0, text: String::new() })
            })
            .collect();
        regions.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(regions
            .into_iter()
            .enumerate()
            .map(|(i, (_, mut region))| {
                region.order = i as u32 + 1;
                region
            })
            .collect())
    }
}
