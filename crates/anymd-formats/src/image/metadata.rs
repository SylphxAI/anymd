use std::io::Cursor;

use exif::{In, Tag, Value};

pub(super) fn exif_facts(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(bytes)) else {
        return Vec::new();
    };
    let text = |tag: Tag| -> Option<String> {
        let field = exif.get_field(tag, In::PRIMARY)?;
        let value = match &field.value {
            Value::Ascii(chunks) => chunks
                .iter()
                .map(|c| {
                    String::from_utf8_lossy(c)
                        .trim_end_matches('\0')
                        .trim()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => field.display_value().with_unit(&exif).to_string(),
        };
        let value = value.trim().trim_matches('"').trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    let mut facts = Vec::new();

    let make = text(Tag::Make);
    let model = text(Tag::Model);
    let camera = match (make, model) {
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model)
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (make, model) => make.or(model),
    };
    if let Some(camera) = camera {
        facts.push(("camera".into(), camera));
    }
    if let Some(lens) = text(Tag::LensModel) {
        facts.push(("lens".into(), lens));
    }
    if let Some(taken) = text(Tag::DateTimeOriginal).or_else(|| text(Tag::DateTime)) {
        facts.push(("taken".into(), taken));
    }
    let exposure: Vec<String> = [
        text(Tag::ExposureTime),
        text(Tag::FNumber),
        text(Tag::PhotographicSensitivity).map(|iso| format!("ISO {iso}")),
        text(Tag::FocalLength),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !exposure.is_empty() {
        facts.push(("exposure".into(), exposure.join(", ")));
    }
    if let Some(orientation) = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .and_then(orientation_text)
    {
        facts.push(("orientation".into(), orientation.into()));
    }
    if let Some(gps) = gps(&exif) {
        facts.push(("gps".into(), gps));
    }
    for (key, tag) in [
        ("description", Tag::ImageDescription),
        ("artist", Tag::Artist),
        ("copyright", Tag::Copyright),
        ("software", Tag::Software),
    ] {
        if let Some(value) = text(tag) {
            facts.push((key.into(), value.chars().take(200).collect()));
        }
    }
    facts
}

fn orientation_text(value: u32) -> Option<&'static str> {
    Some(match value {
        2 => "mirrored horizontally",
        3 => "rotated 180°",
        4 => "mirrored vertically",
        5 => "mirrored, rotated 90° CW",
        6 => "rotated 90° CW",
        7 => "mirrored, rotated 90° CCW",
        8 => "rotated 90° CCW",
        _ => return None,
    })
}

fn gps(exif: &exif::Exif) -> Option<String> {
    let coord = |tag: Tag, reference: Tag, negative: u8| -> Option<f64> {
        let field = exif.get_field(tag, In::PRIMARY)?;
        let Value::Rational(parts) = &field.value else {
            return None;
        };
        let part = |i: usize| {
            parts
                .get(i)
                .filter(|r| r.denom != 0)
                .map(|r| r.to_f64())
                .unwrap_or(0.0)
        };
        let mut degrees = part(0) + part(1) / 60.0 + part(2) / 3600.0;
        let sign = exif
            .get_field(reference, In::PRIMARY)
            .and_then(|f| match &f.value {
                Value::Ascii(chunks) => chunks.first().and_then(|c| c.first().copied()),
                _ => None,
            });
        if sign == Some(negative) {
            degrees = -degrees;
        }
        degrees.is_finite().then_some(degrees)
    };
    let lat = coord(Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S')?;
    let lon = coord(Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W')?;
    let mut out = format!("{lat:.6}, {lon:.6}");
    if let Some(alt) = exif
        .get_field(Tag::GPSAltitude, In::PRIMARY)
        .and_then(|f| match &f.value {
            Value::Rational(r) => r.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
            _ => None,
        })
    {
        out.push_str(&format!(" (altitude {alt:.0} m)"));
    }
    Some(out)
}
