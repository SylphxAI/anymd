use hayro::hayro_interpret::hayro_syntax::DecryptionError;
use image::{GenericImageView, ImageFormat};

use super::geometry::apply_transform;
use super::*;

#[test]
fn cite_check_inverse_matches_real_rotated_cropbox_render() {
    for rotation in [0, 90, 180, 270] {
        let rendered = render_pdf_page(
            fixture_pdf_with_crop_box(rotation, Some("50 60 170 140")),
            1,
            2.0,
            DEFAULT_MAX_RENDER_PIXELS,
        )
        .unwrap();
        let bounds = BoundingBox {
            left: 70.,
            bottom: 75.,
            right: 100.,
            top: 95.,
        };
        let matrix = rendered.pdf_to_pixel_transform();
        let corners = [
            (bounds.left, bounds.bottom),
            (bounds.left, bounds.top),
            (bounds.right, bounds.bottom),
            (bounds.right, bounds.top),
        ]
        .map(|(x, y)| apply_transform(matrix, x, y));
        let pixels = BoundingBox {
            left: corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
            bottom: corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
            right: corners
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max),
            top: corners
                .iter()
                .map(|p| p.1)
                .fold(f64::NEG_INFINITY, f64::max),
        };
        assert_eq!(pdf_box_from_pixel_box(matrix, pixels).unwrap(), bounds);
    }
    assert!(pdf_box_from_pixel_box(
        [0.; 6],
        BoundingBox {
            left: 0.,
            bottom: 0.,
            right: 1.,
            top: 1.
        }
    )
    .is_err());
}

fn fixture_pdf(rotation: i32) -> Vec<u8> {
    fixture_pdf_with_crop_box(rotation, None)
}

fn fixture_pdf_with_crop_box(rotation: i32, crop_box: Option<&str>) -> Vec<u8> {
    let content = "0.1 0.2 0.8 rg\n10 10 100 60 re f\n";
    let media_box = if crop_box.is_some() {
        "0 0 200 200"
    } else {
        "0 0 120 80"
    };
    let crop_box = crop_box
        .map(|value| format!(" /CropBox [{value}]"))
        .unwrap_or_default();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}]{crop_box} /Rotate {rotation} /Resources << >> /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[test]
fn fractional_page_dimensions_keep_the_planned_white_viewport() {
    let bytes = fixture_pdf_with_crop_box(0, Some("0 0 120.25 80.25"));
    let document = RenderDocument::new(bytes).unwrap();
    let planned = document.planned_render_pixel_count(1, 1.01).unwrap();
    assert_eq!(planned, 122 * 82);
    let rendered = document
        .render_page(1, 1.01, planned, DEFAULT_MAX_RENDER_OUTPUT_BYTES)
        .unwrap();
    assert_eq!((rendered.width, rendered.height), (122, 82));
    assert_eq!(rendered.pixel_count, planned);
    let image = image::load_from_memory(&rendered.png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (122, 82));
    assert_eq!(image.get_pixel(121, 81).0, [255, 255, 255, 255]);
    assert_eq!(
        document
            .render_page(1, 1.01, planned - 1, DEFAULT_MAX_RENDER_OUTPUT_BYTES)
            .unwrap_err()
            .code,
        RenderErrorCode::LimitExceeded
    );
}

#[test]
fn renders_fixture_page_to_bounded_png_at_scale_two() {
    let bytes = fixture_pdf(0);
    let planned = RenderDocument::new(bytes.clone())
        .expect("plan fixture")
        .planned_render_pixel_count(1, 2.0)
        .expect("planned pixels");
    let rendered =
        render_pdf_page(bytes, 1, 2.0, DEFAULT_MAX_RENDER_PIXELS).expect("render fixture");
    assert_eq!(rendered.page, 1);
    assert_eq!(rendered.scale, 2.0);
    assert_eq!((rendered.width, rendered.height), (240, 160));
    assert_eq!(
        rendered.pixel_count,
        u64::from(rendered.width) * u64::from(rendered.height)
    );
    assert_eq!(planned, rendered.pixel_count);
    assert!(rendered.pixel_count <= DEFAULT_MAX_RENDER_PIXELS);
    assert_eq!(&rendered.png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(rendered.provenance.engine, RENDER_ENGINE);
    assert_eq!(rendered.provenance.renderer, RENDERER_NAME);

    let decoded = image::load_from_memory_with_format(&rendered.png, ImageFormat::Png)
        .expect("decode rendered PNG");
    assert_eq!(decoded.dimensions(), (rendered.width, rendered.height));
}

#[test]
fn rejects_invalid_dimensions_and_pixel_budget_before_rendering() {
    let bytes = fixture_pdf(0);
    let err =
        render_pdf_page(bytes.clone(), 1, 0.0, DEFAULT_MAX_RENDER_PIXELS).expect_err("zero scale");
    assert_eq!(err.code, RenderErrorCode::InvalidRequest);

    let err = render_pdf_page(bytes, 1, 2.0, 1).expect_err("pixel budget");
    assert_eq!(err.code, RenderErrorCode::LimitExceeded);
    assert!(err.message.contains("exceeding max_pixels_per_page"));
}

#[test]
fn reports_page_rotation_and_rotated_dimensions() {
    let rendered = render_pdf_page(fixture_pdf(90), 1, 2.0, DEFAULT_MAX_RENDER_PIXELS)
        .expect("render rotated fixture");
    assert_eq!(rendered.rotation, 90);
    assert_eq!((rendered.width, rendered.height), (160, 240));
}

#[test]
fn reusable_document_reports_pages_and_enforces_selected_page_limit() {
    let document = RenderDocument::new(fixture_pdf(0)).expect("parse fixture once");
    assert_eq!(document.page_count(), 1);
    assert_eq!(pdf_page_count(fixture_pdf(0)).unwrap(), 1);
    let pages = document
        .render_selected_pages(
            &[1],
            1.0,
            RenderLimits {
                max_pages: 1,
                ..RenderLimits::default()
            },
        )
        .expect("render selected page");
    assert_eq!(pages.len(), 1);

    let error = document
        .render_selected_pages(
            &[1, 1],
            1.0,
            RenderLimits {
                max_pages: 1,
                ..RenderLimits::default()
            },
        )
        .expect_err("selected page count must be bounded");
    assert_eq!(error.code, RenderErrorCode::LimitExceeded);
}

#[test]
fn exact_page_transform_handles_all_right_angle_rotations() {
    let expected = [
        (
            0,
            CropPixels {
                left: 10,
                top: 60,
                width: 20,
                height: 10,
            },
        ),
        (
            90,
            CropPixels {
                left: 10,
                top: 10,
                width: 10,
                height: 20,
            },
        ),
        (
            180,
            CropPixels {
                left: 90,
                top: 10,
                width: 20,
                height: 10,
            },
        ),
        (
            270,
            CropPixels {
                left: 60,
                top: 90,
                width: 10,
                height: 20,
            },
        ),
    ];
    let region = BoundingBox {
        left: 10.0,
        bottom: 10.0,
        right: 30.0,
        top: 20.0,
    };
    for (rotation, expected_crop) in expected {
        let rendered =
            render_pdf_page(fixture_pdf(rotation), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS).unwrap();
        assert_eq!(
            crop_pixels_for_rendered_page(&rendered, region, 0.0).unwrap(),
            expected_crop,
            "rotation {rotation}"
        );
    }
}

#[test]
fn exact_page_transform_accounts_for_nonzero_crop_box_origin() {
    let rendered = render_pdf_page(
        fixture_pdf_with_crop_box(0, Some("50 60 170 140")),
        1,
        1.0,
        DEFAULT_MAX_RENDER_PIXELS,
    )
    .expect("render cropped page");
    assert_eq!((rendered.width, rendered.height), (120, 80));
    assert_eq!(
        crop_pixels_for_rendered_page(
            &rendered,
            BoundingBox {
                left: 60.0,
                bottom: 70.0,
                right: 80.0,
                top: 90.0,
            },
            0.0,
        )
        .unwrap(),
        CropPixels {
            left: 10,
            top: 50,
            width: 20,
            height: 20,
        }
    );
}

#[test]
fn malformed_pdf_and_output_limit_return_structured_errors_without_panics() {
    let malformed = render_pdf_page(b"not a PDF".to_vec(), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS)
        .expect_err("malformed input");
    assert_eq!(malformed.code, RenderErrorCode::InvalidPdf);

    let document = RenderDocument::new(fixture_pdf(0)).unwrap();
    let limited = document
        .render_page(1, 1.0, DEFAULT_MAX_RENDER_PIXELS, 16)
        .expect_err("encoded output limit");
    assert_eq!(limited.code, RenderErrorCode::LimitExceeded);
}

#[test]
fn encrypted_pdf_load_errors_are_classified_without_exposing_parser_details() {
    let error = map_load_error(LoadPdfError::Decryption(DecryptionError::PasswordProtected));
    assert_eq!(error.code, RenderErrorCode::EncryptedPdf);
    assert!(error.message.contains("Encrypted PDF"));
}

#[test]
fn rendered_and_cropped_pngs_are_deterministic_rgba8() {
    let rendered_a = render_pdf_page(fixture_pdf(0), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS).unwrap();
    let rendered_b = render_pdf_page(fixture_pdf(0), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS).unwrap();
    assert_eq!(rendered_a.png, rendered_b.png);
    assert_eq!(
        image::load_from_memory_with_format(&rendered_a.png, ImageFormat::Png)
            .unwrap()
            .color(),
        image::ColorType::Rgba8
    );

    let (crop, crop_png) = crop_rendered_page_region(
        &rendered_a,
        BoundingBox {
            left: 10.0,
            bottom: 10.0,
            right: 30.0,
            top: 20.0,
        },
        0.0,
        DEFAULT_MAX_RENDER_OUTPUT_BYTES,
    )
    .unwrap();
    let decoded = image::load_from_memory_with_format(&crop_png, ImageFormat::Png).unwrap();
    assert_eq!(decoded.dimensions(), (crop.width, crop.height));
    assert_eq!(decoded.color(), image::ColorType::Rgba8);
}

#[test]
fn matches_ts_bottom_left_to_top_left_crop_math_with_clamp_and_padding() {
    assert_eq!(
        crop_pixels_for_bounding_box(
            10,
            10,
            1.0,
            0,
            BoundingBox {
                left: 2.0,
                bottom: 2.0,
                right: 6.0,
                top: 6.0
            },
            1.0,
        )
        .expect("crop"),
        CropPixels {
            left: 1,
            top: 3,
            width: 6,
            height: 6
        }
    );
    assert_eq!(
        crop_pixels_for_bounding_box(
            10,
            10,
            1.0,
            0,
            BoundingBox {
                left: -4.0,
                bottom: -3.0,
                right: 3.0,
                top: 4.0
            },
            0.0,
        )
        .expect("clamped crop"),
        CropPixels {
            left: 0,
            top: 6,
            width: 3,
            height: 4
        }
    );
}

#[test]
fn rejects_invalid_or_non_intersecting_bounding_boxes() {
    let invalid = crop_pixels_for_bounding_box(
        10,
        10,
        1.0,
        0,
        BoundingBox {
            left: 4.0,
            bottom: 2.0,
            right: 4.0,
            top: 6.0,
        },
        0.0,
    )
    .expect_err("invalid box");
    assert_eq!(invalid.code, RenderErrorCode::InvalidRequest);

    let outside = crop_pixels_for_bounding_box(
        10,
        10,
        1.0,
        0,
        BoundingBox {
            left: 20.0,
            bottom: 20.0,
            right: 30.0,
            top: 30.0,
        },
        0.0,
    )
    .expect_err("non-intersecting box");
    assert!(outside.message.contains("does not intersect"));

    let rotated = crop_pixels_for_bounding_box(
        80,
        120,
        1.0,
        90,
        BoundingBox {
            left: 2.0,
            bottom: 2.0,
            right: 6.0,
            top: 6.0,
        },
        0.0,
    )
    .expect("rotated crop");
    assert_eq!(
        rotated,
        CropPixels {
            left: 2,
            top: 2,
            width: 4,
            height: 4,
        }
    );
}

#[test]
fn crops_fixture_render_to_valid_png_with_deterministic_dimensions() {
    let rendered =
        render_pdf_page(fixture_pdf(0), 1, 2.0, DEFAULT_MAX_RENDER_PIXELS).expect("render fixture");
    let crop = CropPixels {
        left: rendered.width / 4,
        top: rendered.height / 4,
        width: rendered.width / 2,
        height: rendered.height / 2,
    };
    let png = crop_rendered_page_png(&rendered.png, rendered.width, rendered.height, crop)
        .expect("crop PNG");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let decoded =
        image::load_from_memory_with_format(&png, ImageFormat::Png).expect("decode crop PNG");
    assert_eq!(decoded.dimensions(), (crop.width, crop.height));
}
