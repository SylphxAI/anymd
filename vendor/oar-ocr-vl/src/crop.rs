//! Cropping a page image down to a detected region.

use crate::error::{Error, ProcessingStage};
use crate::geometry::BoundingBox;
use image::RgbImage;

/// Crops the axis-aligned extent of `bbox` out of `image`, clamped to the image
/// bounds.
pub fn crop_bounding_box(image: &RgbImage, bbox: &BoundingBox) -> Result<RgbImage, Error> {
    if bbox.points.is_empty() {
        return Err(crop_error("empty bounding box".to_string()));
    }

    let x1 = (bbox.x_min().max(0.0) as u32).min(image.width().saturating_sub(1));
    let y1 = (bbox.y_min().max(0.0) as u32).min(image.height().saturating_sub(1));
    let x2 = (bbox.x_max().max(0.0) as u32).min(image.width());
    let y2 = (bbox.y_max().max(0.0) as u32).min(image.height());

    if x2 <= x1 || y2 <= y1 {
        return Err(crop_error(format!(
            "invalid crop region: ({x1}, {y1}) to ({x2}, {y2})"
        )));
    }

    Ok(image::imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image())
}

fn crop_error(context: String) -> Error {
    Error::Processing {
        kind: ProcessingStage::ImageProcessing,
        context,
        source: Box::new(Error::invalid_input("bounding box crop")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_the_requested_region() {
        let image = RgbImage::new(100, 80);
        let crop = crop_bounding_box(&image, &BoundingBox::from_coords(10.0, 20.0, 40.0, 60.0))
            .expect("crop succeeds");
        assert_eq!(crop.dimensions(), (30, 40));
    }

    /// Boxes reaching past the edges are clamped rather than rejected.
    #[test]
    fn clamps_to_the_image_bounds() {
        let image = RgbImage::new(50, 50);
        let crop = crop_bounding_box(
            &image,
            &BoundingBox::from_coords(-10.0, -10.0, 999.0, 999.0),
        )
        .expect("crop succeeds");
        assert_eq!(crop.dimensions(), (50, 50));
    }

    #[test]
    fn rejects_degenerate_boxes() {
        let image = RgbImage::new(50, 50);
        assert!(crop_bounding_box(&image, &BoundingBox::new(Vec::new())).is_err());
        assert!(
            crop_bounding_box(&image, &BoundingBox::from_coords(10.0, 10.0, 10.0, 10.0)).is_err()
        );
    }
}
