use std::path::Path;
use std::time::Instant;

use anymd_formats::video::timeline::{DecodedFrame, FrameMetadata, Status, TimelineOptions};

use super::{enrich, validate_observation};
use crate::video_evidence::{
    sections, tests::fixture, FrameContext, FrameDescription, RasterObservation, RasterRegion,
};

#[test]
fn observation_geometry_never_assumes_pdf_page() {
    let frame = FrameMetadata {
        requested_ms: 0,
        actual_ms: 17,
        decoded_pts: 17,
        decoded_pts_us: 17_000,
        stream: 0,
        time_base: "1/1000".into(),
        width: 100,
        height: 50,
        sha256: "hash".into(),
    };
    let mut observation = RasterObservation {
        provider: "ocr".into(),
        model_revision: Some("revision".into()),
        coordinate_space: "frame_pixels_top_left".into(),
        regions: vec![RasterRegion {
            text: "observed".into(),
            left: 0.,
            top: 0.,
            right: 100.,
            bottom: 50.,
            geometry_level: "ocr_region".into(),
        }],
        truncated: false,
    };
    assert!(validate_observation(&observation, &frame).is_ok());
    observation.regions[0].right = 101.;
    assert!(validate_observation(&observation, &frame).is_err());
    observation.coordinate_space = "pdf_points".into();
    assert!(validate_observation(&observation, &frame).is_err());
}

struct InertContext {
    exhausted: bool,
    local_caption: bool,
    worker_calls: usize,
}
impl FrameContext for InertContext {
    fn ensure_available(&self, _deadline: Instant) -> Result<(), String> {
        if self.exhausted {
            Err("shared request budget exhausted".into())
        } else {
            Ok(())
        }
    }
    fn charge_frame(&mut self, _pixels: u64, _bytes: u64) -> Result<(), String> {
        Ok(())
    }
    fn ocr(
        &mut self,
        _frame: &DecodedFrame,
        _deadline: Instant,
    ) -> Result<Option<RasterObservation>, String> {
        self.worker_calls += 1;
        Ok(None)
    }
    fn local_caption_available(&self) -> bool {
        self.local_caption
    }
    fn caption_cached(
        &mut self,
        _hash: &str,
        _scene: &str,
        _frame: &DecodedFrame,
        _deadline: Instant,
    ) -> Result<Option<FrameDescription>, String> {
        self.worker_calls += 1;
        Ok(None)
    }
}
#[test]
fn absent_local_caption_does_not_decode_or_invent_description() {
    let evidence = fixture();
    let original_headings: Vec<_> = sections(&evidence).into_iter().map(|s| s.label).collect();
    let options = TimelineOptions {
        start_ms: 0,
        end_ms: 2000,
        max_scenes: 20,
        caption: true,
    };
    let mut context = InertContext {
        exhausted: false,
        local_caption: false,
        worker_calls: 0,
    };
    let result = enrich(
        Path::new("not-opened"),
        evidence.timeline,
        &options,
        false,
        Instant::now(),
        &mut context,
    )
    .unwrap();
    assert_eq!(context.worker_calls, 0);
    assert_eq!(
        result.timeline.components.caption.status,
        Status::Unavailable
    );
    assert!(result.descriptions.is_empty());
    assert_eq!(
        original_headings,
        sections(&result)
            .into_iter()
            .map(|s| s.label)
            .collect::<Vec<_>>()
    );
    let text = sections(&result)
        .into_iter()
        .map(|s| s.markdown)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches("cross-cut unique text").count(), 1);
}
#[test]
fn exhausted_request_starts_no_later_representative_or_provider() {
    let evidence = fixture();
    let options = TimelineOptions {
        start_ms: 0,
        end_ms: 2000,
        max_scenes: 20,
        caption: true,
    };
    let mut context = InertContext {
        exhausted: true,
        local_caption: true,
        worker_calls: 0,
    };
    let result = enrich(
        Path::new("not-opened"),
        evidence.timeline,
        &options,
        true,
        Instant::now(),
        &mut context,
    )
    .unwrap();
    assert_eq!(context.worker_calls, 0);
    assert_eq!(result.timeline.components.ocr.status, Status::Partial);
    assert_eq!(result.timeline.components.caption.status, Status::Partial);
    assert!(result
        .timeline
        .scenes
        .iter()
        .all(|s| s.representative.is_none()));
}
