//! Preflight dimensions and rasterize a PDF page to a bounded PNG.

use hayro::hayro_interpret::hayro_syntax::page::{Page, Rotation};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{render_into, RenderCache, RenderSettings};

use super::png::encode_rgba_png_bounded;
use super::{RenderError, RenderProvenance, RenderedPage, RENDERER_NAME, RENDER_ENGINE};

pub(super) fn planned_dimensions(
    page: &Page<'_>,
    scale: f32,
    page_number: usize,
) -> Result<(u32, u32, u64), RenderError> {
    let (page_width, page_height) = page.render_dimensions();
    let width = checked_render_dimension(page_width, scale, "width", page_number)?;
    let height = checked_render_dimension(page_height, scale, "height", page_number)?;
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| RenderError::limit_exceeded("Rendered page dimensions overflow."))?;
    Ok((width, height, pixel_count))
}

pub(super) fn rasterize_page(
    page: &Page<'_>,
    page_number: usize,
    scale: f32,
    max_pixels: u64,
    max_output_bytes: u64,
) -> Result<RenderedPage, RenderError> {
    let (width, height, pixel_count) = planned_dimensions(page, scale, page_number)?;
    if pixel_count > max_pixels {
        return Err(RenderError::limit_exceeded(format!(
            "Page {page_number} would render {pixel_count} pixels at scale {scale}, exceeding max_pixels_per_page {max_pixels}. Lower scale or raise max_pixels_per_page."
        )));
    }

    let viewport_width = u16::try_from(width).map_err(|_| {
        RenderError::limit_exceeded(format!(
            "Page {page_number} render width {width} exceeds renderer limit {}.",
            u16::MAX
        ))
    })?;
    let viewport_height = u16::try_from(height).map_err(|_| {
        RenderError::limit_exceeded(format!(
            "Page {page_number} render height {height} exceeds renderer limit {}.",
            u16::MAX
        ))
    })?;

    let mut pdf_to_pixel = page.initial_transform(true).as_coeffs();
    for coefficient in &mut pdf_to_pixel {
        *coefficient *= f64::from(scale);
    }

    let pixmap = rasterize_on_white(
        page,
        pdf_to_pixel,
        (viewport_width, viewport_height),
        &render_settings(),
    );
    let rgba = pixmap.data_as_u8_slice();
    ensure_opaque(rgba)?;
    let png = encode_rgba_png_bounded(rgba, width, height, max_output_bytes)?;

    Ok(RenderedPage {
        page: page_number,
        width,
        height,
        scale,
        pixel_count,
        rotation: rotation_degrees(page.rotation()),
        format: "png",
        mime_type: "image/png",
        provenance: RenderProvenance {
            engine: RENDER_ENGINE,
            renderer: RENDERER_NAME,
            source: "page-render",
        },
        png,
        pdf_to_pixel,
    })
}

/// hayro's glyph cache paints text as anti-aliased bitmaps snapped to whole
/// pixels vertically and quarter pixels horizontally. vello_cpu 0.3's u8
/// compositor drops one alpha step where such a translucent bitmap pixel meets
/// partial clip coverage, so opaque text crossing an anti-aliased clip edge
/// left alpha-254 pixels on the white page. Filling the glyph outlines keeps
/// text at its source position and the page opaque.
pub(super) fn render_settings() -> RenderSettings {
    RenderSettings {
        glyph_caching: false,
        ..RenderSettings::default()
    }
}

pub(super) fn rasterize_on_white(
    page: &Page<'_>,
    pdf_to_pixel: [f64; 6],
    (width, height): (u16, u16),
    settings: &RenderSettings,
) -> Pixmap {
    // hayro's scale-only helper truncates fractional dimensions. Keep the
    // preflight's rounded-up viewport so allocation and PNG dimensions agree.
    let mut context = RenderContext::new(width, height);
    render_into(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        settings,
        &mut context,
        hayro::kurbo::Affine::new(pdf_to_pixel),
    );
    context.flush();
    let mut pixmap = Pixmap::new(width, height);
    context.render_with(
        &mut pixmap,
        &mut Resources::default(),
        RasterizerSettings {
            target_init: TargetInit::Clear(WHITE),
            ..Default::default()
        },
    );
    pixmap
}

/// Every pixel of a render composited onto opaque white must stay opaque.
pub(super) fn ensure_opaque(rgba: &[u8]) -> Result<(), RenderError> {
    if rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 255) {
        return Err(RenderError::encode_failed(
            "White-background render unexpectedly contained transparent pixels.",
        ));
    }
    Ok(())
}

fn rotation_degrees(rotation: Rotation) -> i32 {
    match rotation {
        Rotation::None => 0,
        Rotation::Horizontal => 90,
        Rotation::Flipped => 180,
        Rotation::FlippedHorizontal => 270,
    }
}

fn checked_render_dimension(
    page_points: f32,
    scale: f32,
    axis: &str,
    page_number: usize,
) -> Result<u32, RenderError> {
    let scaled = f64::from(page_points) * f64::from(scale);
    if !scaled.is_finite() || scaled <= 0.0 || scaled > f64::from(u32::MAX) {
        return Err(RenderError::invalid_request(format!(
            "Page {page_number} has invalid render {axis}."
        )));
    }
    Ok(scaled.ceil() as u32)
}
