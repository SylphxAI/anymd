use super::client::{request, RequestOptions};
use crate::schema::{InspectArgs, InspectOperation};
use crate::source_access::SourceAccessPolicy;

pub(crate) fn inspect(
    args: InspectArgs,
    policy: &SourceAccessPolicy,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let run = || -> Result<rmcp::model::CallToolResult, String> {
        if args.sources.len() != 1 {
            return Err("video operation requires exactly one source".into());
        }
        let source = &args.sources[0];
        let value = serde_json::to_value(source).map_err(|e| e.to_string())?;
        if value.as_object().is_some_and(|v| {
            v.iter()
                .any(|(k, v)| k != "path" && k != "url" && !v.is_null())
        }) {
            return Err("video source does not accept PDF page/region options".into());
        }
        let spec = match (&source.path, &source.url) {
            (Some(path), None) => path.as_str(),
            (None, Some(url)) => url.as_str(),
            _ => return Err("source requires path or URL, not both".into()),
        };
        if args.profile.is_some()
            || args.sample_pages.is_some()
            || args.scale.is_some()
            || args.max_pages.is_some()
            || args.max_regions.is_some()
            || args.max_pixels_per_page.is_some()
            || args.languages.is_some()
            || args.include_metadata.is_some()
        {
            return Err("PDF-only options are not applicable to video".into());
        }
        let timeline = matches!(args.operation, InspectOperation::VideoTimeline);
        if timeline
            && (args.timeline.is_none()
                || args.timestamps_ms.is_some()
                || args.include_image.unwrap_or(false))
        {
            return Err("video_timeline requires timeline and returns no image blocks".into());
        }
        if !timeline
            && (args.timeline.is_some()
                || args.timestamps_ms.is_none()
                || args.transcript.is_some()
                || args.ocr.is_some())
        {
            return Err("render_frame requires timestamps only".into());
        }
        let max = args.max_output_chars.unwrap_or(200_000) as usize;
        if !(1000..=1_000_000).contains(&max) {
            return Err("invalid max_output_chars".into());
        }
        let mut payload = request(
            spec,
            RequestOptions {
                selection: args.timeline,
                timestamps: args.timestamps_ms,
                transcript: args.transcript.unwrap_or(false),
                ocr: args.ocr,
                expected: args.expected_source_sha256.as_deref(),
                timeout: args.timeout_ms.unwrap_or(60_000) as u64,
            },
            policy,
        )?;
        let mut images = Vec::new();
        if !timeline && args.include_image.unwrap_or(false) {
            for frame in payload["frames"].as_array().into_iter().flatten() {
                if let Some(path) = frame["image_path"].as_str() {
                    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                    use base64::Engine;
                    images.push(rmcp::model::ContentBlock::image(
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                        "image/png",
                    ));
                }
            }
        }
        if let Some(frames) = payload["frames"].as_array_mut() {
            for frame in frames {
                if let Some(object) = frame.as_object_mut() {
                    object.remove("image_path");
                }
            }
        }
        if serde_json::to_string(&payload)
            .map_err(|e| e.to_string())?
            .encode_utf16()
            .count()
            > max
        {
            return Err("video response exceeds output budget".into());
        }
        let source_hash = payload["source_sha256"]
            .as_str()
            .or_else(|| payload["timeline"]["source_sha256"].as_str())
            .map(str::to_string);
        let payload = crate::evidence::attach_evidence(
            "inspect",
            Some(if timeline {
                "video_timeline"
            } else {
                "render_frame"
            }),
            &[source.as_pdf_source()],
            "local-video",
            source_hash,
            vec![],
            payload,
        );
        let mut result = rmcp::model::CallToolResult::structured(payload);
        result.content.extend(images);
        Ok(result)
    };
    run().map_err(|message| rmcp::ErrorData::invalid_params(message, None))
}
