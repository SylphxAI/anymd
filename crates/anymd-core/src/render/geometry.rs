//! PDF and pixel coordinate transforms for page regions.

use serde::{Deserialize, Serialize};

use super::{RenderError, RenderedPage};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropPixels {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

/// Inverse of the render owner's transform. Pixel boxes use a top-left origin.
/// All four corners are projected, so rotated/CropBox pages do not use a
/// scale-only approximation. Invalid/singular transforms fail closed.
pub fn pdf_box_from_pixel_box(
    transform: [f64; 6],
    bounds: BoundingBox,
) -> Result<BoundingBox, RenderError> {
    let [a, b, c, d, e, f] = transform;
    let determinant = a * d - b * c;
    if !transform.into_iter().all(f64::is_finite)
        || !determinant.is_finite()
        || determinant.abs() < f64::EPSILON
    {
        return Err(RenderError::invalid_request("Invalid render transform."));
    }
    if ![bounds.left, bounds.bottom, bounds.right, bounds.top]
        .into_iter()
        .all(f64::is_finite)
        || bounds.right <= bounds.left
        || bounds.top <= bounds.bottom
    {
        return Err(RenderError::invalid_request("Invalid pixel bounding box."));
    }
    let corners = [
        (bounds.left, bounds.bottom),
        (bounds.left, bounds.top),
        (bounds.right, bounds.bottom),
        (bounds.right, bounds.top),
    ];
    let points = corners.map(|(x, y)| {
        (
            (d * (x - e) - c * (y - f)) / determinant,
            (-b * (x - e) + a * (y - f)) / determinant,
        )
    });
    Ok(BoundingBox {
        left: points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        bottom: points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        right: points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        top: points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    })
}

/// Convert a PDF bottom-left-origin bounding box into a clamped top-left pixel crop.
///
/// This compatibility entrypoint assumes a zero-origin page box. Use
/// [`crop_pixels_for_rendered_page`] when a parsed page is available; it carries
/// the exact CropBox offset and Hayro render transform.
pub fn crop_pixels_for_bounding_box(
    page_width: u32,
    page_height: u32,
    scale: f32,
    page_rotation: i32,
    bounding_box: BoundingBox,
    padding: f64,
) -> Result<CropPixels, RenderError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RenderError::invalid_request(
            "scale must be a finite positive number.",
        ));
    }
    if !padding.is_finite() || padding < 0.0 {
        return Err(RenderError::invalid_request(
            "padding must be a finite non-negative number.",
        ));
    }
    let scale = f64::from(scale);
    let transform = match page_rotation.rem_euclid(360) {
        0 => [scale, 0.0, 0.0, -scale, 0.0, f64::from(page_height)],
        90 => [0.0, scale, scale, 0.0, 0.0, 0.0],
        180 => [-scale, 0.0, 0.0, scale, f64::from(page_width), 0.0],
        270 => [
            0.0,
            -scale,
            -scale,
            0.0,
            f64::from(page_width),
            f64::from(page_height),
        ],
        other => {
            return Err(RenderError::invalid_request(format!(
                "Unsupported page rotation {other}; expected a multiple of 90 degrees."
            )))
        }
    };
    crop_pixels_for_transform(page_width, page_height, transform, bounding_box, padding)
}

/// Convert PDF coordinates using the exact transform used to render this page.
pub fn crop_pixels_for_rendered_page(
    page: &RenderedPage,
    bounding_box: BoundingBox,
    padding: f64,
) -> Result<CropPixels, RenderError> {
    crop_pixels_for_transform(
        page.width,
        page.height,
        page.pdf_to_pixel,
        bounding_box,
        padding,
    )
}

fn crop_pixels_for_transform(
    page_width: u32,
    page_height: u32,
    transform: [f64; 6],
    bounding_box: BoundingBox,
    padding: f64,
) -> Result<CropPixels, RenderError> {
    if !padding.is_finite() || padding < 0.0 {
        return Err(RenderError::invalid_request(
            "padding must be a finite non-negative number.",
        ));
    }
    if !valid_bounding_box(bounding_box) {
        return Err(RenderError::invalid_request(
            "Region bounding_box must have right > left and top > bottom.",
        ));
    }
    let expanded = BoundingBox {
        left: bounding_box.left - padding,
        bottom: bounding_box.bottom - padding,
        right: bounding_box.right + padding,
        top: bounding_box.top + padding,
    };
    if ![expanded.left, expanded.bottom, expanded.right, expanded.top]
        .iter()
        .all(|value| value.is_finite())
    {
        return Err(RenderError::invalid_request(
            "Region bounding_box plus padding must remain finite.",
        ));
    }

    let corners = [
        (expanded.left, expanded.bottom),
        (expanded.left, expanded.top),
        (expanded.right, expanded.bottom),
        (expanded.right, expanded.top),
    ];
    let transformed = corners.map(|(x, y)| apply_transform(transform, x, y));
    let left = transformed
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::INFINITY, f64::min)
        .floor();
    let right = transformed
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();
    let top = transformed
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min)
        .floor();
    let bottom = transformed
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();

    let clamped_left = clamp_pixel(left, page_width);
    let clamped_top = clamp_pixel(top, page_height);
    let clamped_right = clamp_pixel(right, page_width);
    let clamped_bottom = clamp_pixel(bottom, page_height);
    let width = clamped_right.saturating_sub(clamped_left);
    let height = clamped_bottom.saturating_sub(clamped_top);
    if width == 0 || height == 0 {
        return Err(RenderError::invalid_request(
            "Region bounding_box does not intersect the rendered page.",
        ));
    }

    Ok(CropPixels {
        left: clamped_left,
        top: clamped_top,
        width,
        height,
    })
}

pub(super) fn apply_transform(transform: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (
        transform[0] * x + transform[2] * y + transform[4],
        transform[1] * x + transform[3] * y + transform[5],
    )
}

fn valid_bounding_box(box_: BoundingBox) -> bool {
    box_.left.is_finite()
        && box_.bottom.is_finite()
        && box_.right.is_finite()
        && box_.top.is_finite()
        && box_.right > box_.left
        && box_.top > box_.bottom
}

fn clamp_pixel(value: f64, maximum: u32) -> u32 {
    value.max(0.0).min(f64::from(maximum)) as u32
}
