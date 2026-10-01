use super::config::HpdParsingConfig;
use crate::error::Error;
use crate::utils::image::{image_to_chw, patchify_merge_grouped};
use candle_core::{DType, Device, Tensor};
use image::{RgbImage, imageops::FilterType};

const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

#[derive(Debug, Clone)]
pub struct HpdImageInputs {
    /// Flattened row-major image patches: `(tiles, patches, 3 * patch²)`.
    pub pixel_patches: Tensor,
    pub num_tiles: usize,
}

pub fn preprocess_image(
    image: &RgbImage,
    cfg: &HpdParsingConfig,
    device: &Device,
    dtype: DType,
) -> Result<HpdImageInputs, Error> {
    if image.width() == 0 || image.height() == 0 {
        return Err(Error::InvalidInput {
            message: "HPD-Parsing cannot process an empty image".to_string(),
        });
    }
    let size = cfg.force_image_size as u32;
    let mut max_blocks = cfg.max_dynamic_patch;
    // This intentionally mirrors the released MAX_PATCHES_WITH_RESIZE path:
    // one extra grid slot is considered before the thumbnail is appended.
    if cfg.use_thumbnail && max_blocks != 1 {
        max_blocks += 1;
    }
    let ratios = target_ratios(cfg.min_dynamic_patch, max_blocks);
    let (cols, rows) = closest_ratio(image.width(), image.height(), size, &ratios);
    let resized = image::imageops::resize(
        image,
        size * cols as u32,
        size * rows as u32,
        FilterType::CatmullRom,
    );
    let mut tiles = Vec::with_capacity(cols * rows + usize::from(cfg.use_thumbnail));
    for row in 0..rows {
        for col in 0..cols {
            tiles.push(
                image::imageops::crop_imm(
                    &resized,
                    col as u32 * size,
                    row as u32 * size,
                    size,
                    size,
                )
                .to_image(),
            );
        }
    }
    if cfg.use_thumbnail && cols * rows != 1 {
        tiles.push(image::imageops::resize(
            image,
            size,
            size,
            FilterType::CatmullRom,
        ));
    }

    let patch = cfg.vision_config.patch_size;
    let grid = cfg.force_image_size / patch;
    let patch_dim = cfg.vision_config.num_channels * patch * patch;
    let patches_per_tile = grid * grid;
    let mut all = Vec::with_capacity(tiles.len() * patches_per_tile * patch_dim);
    for tile in &tiles {
        let frame = image_to_chw(tile, &IMAGENET_MEAN, &IMAGENET_STD, Some(1.0 / 255.0));
        all.extend(patchify_merge_grouped(
            &[frame.as_slice()],
            3,
            cfg.force_image_size,
            cfg.force_image_size,
            1,
            grid,
            grid,
            patch,
            1,
            1,
        ));
    }
    let num_tiles = tiles.len();
    let pixel_patches = Tensor::from_vec(all, (num_tiles, patches_per_tile, patch_dim), device)
        .and_then(|tensor| tensor.to_dtype(dtype))
        .map_err(|e| {
            crate::utils::candle_to_ocr_inference("HPD-Parsing", "create image patches", e)
        })?;
    Ok(HpdImageInputs {
        pixel_patches,
        num_tiles,
    })
}

fn target_ratios(min_blocks: usize, max_blocks: usize) -> Vec<(usize, usize)> {
    let mut ratios = Vec::new();
    for n in min_blocks..=max_blocks {
        for cols in 1..=n {
            for rows in 1..=n {
                let blocks = cols * rows;
                if blocks >= min_blocks && blocks <= max_blocks && !ratios.contains(&(cols, rows)) {
                    ratios.push((cols, rows));
                }
            }
        }
    }
    ratios.sort_by_key(|&(cols, rows)| (cols * rows, cols, rows));
    ratios
}

fn closest_ratio(
    width: u32,
    height: u32,
    image_size: u32,
    ratios: &[(usize, usize)],
) -> (usize, usize) {
    let aspect = width as f64 / height as f64;
    let area = width as f64 * height as f64;
    let mut candidates = ratios
        .iter()
        .copied()
        .filter_map(|ratio @ (cols, rows)| {
            let ar_diff = (aspect - cols as f64 / rows as f64).abs();
            (ar_diff <= 0.2).then(|| {
                let target_area = (image_size as f64).powi(2) * (cols * rows) as f64;
                (ratio, (area - target_area).abs(), ar_diff)
            })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates = ratios
            .iter()
            .copied()
            .map(|ratio @ (cols, rows)| {
                let ar_diff = (aspect - cols as f64 / rows as f64).abs();
                let target_area = (image_size as f64).powi(2) * (cols * rows) as f64;
                (ratio, (area - target_area).abs(), ar_diff)
            })
            .collect();
    }
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
    candidates
        .into_iter()
        .take(3)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|entry| entry.0)
        .unwrap_or((1, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_are_unique_and_area_sorted() {
        let ratios = target_ratios(1, 4);
        assert_eq!(ratios.len(), 8);
        assert!(
            ratios
                .windows(2)
                .all(|pair| { pair[0].0 * pair[0].1 <= pair[1].0 * pair[1].1 })
        );
    }

    #[test]
    fn ratio_selection_tracks_portrait_and_landscape() {
        let ratios = target_ratios(1, 25);
        let landscape = closest_ratio(1800, 900, 448, &ratios);
        let portrait = closest_ratio(900, 1800, 448, &ratios);
        assert!(landscape.0 > landscape.1);
        assert!(portrait.1 > portrait.0);
    }

    #[test]
    fn ratio_selection_matches_official_fixture_choices() {
        let ratios = target_ratios(1, 25);
        assert_eq!(closest_ratio(514, 64, 448, &ratios), (8, 1));
        assert_eq!(closest_ratio(760, 865, 448, &ratios), (2, 2));
        assert_eq!(closest_ratio(248, 193, 448, &ratios), (5, 4));
        assert_eq!(closest_ratio(720, 1150, 448, &ratios), (2, 3));
    }
}
