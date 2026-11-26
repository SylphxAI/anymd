use anymd_formats::video::timeline::{self, Component, FrameMetadata, Timeline};

use super::{
    sections, FrameDescription, RasterObservation, RasterRegion, SampledDescription, SampledOcr,
    VideoEvidence,
};

pub(crate) fn fixture() -> VideoEvidence {
    use timeline::{Components, Cue, MediaClock, Scene, TimeRange};
    VideoEvidence {
        timeline: Timeline {
            version: timeline::POLICY.into(),
            source_sha256: "fixture-hash".into(),
            requested: TimeRange {
                start_ms: 0,
                end_ms: 2000,
            },
            processed: Some(TimeRange {
                start_ms: 0,
                end_ms: 2000,
            }),
            clock: Some(MediaClock {
                origin_pts_us: Some(5_000_000),
                video_start_pts_us: Some(5_000_000),
                audio_start_pts_us: None,
                video_stream: 0,
                video_time_base: "1/1000".into(),
                audio_stream: None,
            }),
            scenes: vec![
                Scene {
                    id: "scene-0".into(),
                    start_ms: 0,
                    end_ms: 1000,
                    detection: "initial_window_scene".into(),
                    representative: None,
                },
                Scene {
                    id: "scene-1000".into(),
                    start_ms: 1000,
                    end_ms: 2000,
                    detection: "detected_cut".into(),
                    representative: None,
                },
            ],
            chapters: vec![],
            cues: vec![Cue {
                track: "en".into(),
                start_ms: 500,
                end_ms: 1500,
                text: "cross-cut unique text".into(),
                timing: "subtitle_cue".into(),
                provider: "srt_webvtt".into(),
            }],
            components: Components {
                scenes: Component::ok(),
                chapters: Component::ok(),
                subtitles: Component::ok(),
                transcript: Component::not_requested(),
                ocr: Component::not_requested(),
                caption: Component::not_requested(),
            },
        },
        ocr_observations: vec![],
        descriptions: vec![],
    }
}

fn node_ids(evidence: &VideoEvidence) -> Vec<(String, String)> {
    let opened = crate::document::Opened::from_video("video", evidence);
    opened
        .build_outline()
        .unwrap()
        .nodes
        .into_iter()
        .map(|n| (n.id, n.title))
        .collect()
}
#[test]

fn generated_text_cannot_change_outline_node_ids_and_chapters_precede_cues() {
    let mut metadata_only = fixture();
    metadata_only.timeline.chapters.push(timeline::Chapter {
        id: "chapter-0".into(),
        start_ms: 0,
        end_ms: 1000,
        title: Some("# Chapter".into()),
    });
    let labels: Vec<_> = sections(&metadata_only)
        .into_iter()
        .map(|s| s.label)
        .collect();
    assert_eq!(
        labels,
        [
            "scene scene-0: 0–1000 ms",
            "scene scene-1000: 1000–2000 ms",
            "chapter chapter-0",
            "timeline cues"
        ]
    );
    let mut enriched = metadata_only.clone();
    let frame = FrameMetadata {
        requested_ms: 0,
        actual_ms: 0,
        decoded_pts: 0,
        decoded_pts_us: 0,
        stream: 0,
        time_base: "1/1000".into(),
        width: 10,
        height: 10,
        sha256: "hash".into(),
    };
    enriched.descriptions.push(SampledDescription {
        scene_id: "scene-0".into(),
        frame: frame.clone(),
        description: FrameDescription {
            text: "# X\n---\n````\n## Y".into(),
            provider: "external-command".into(),
            model_revision: None,
            truncated: false,
        },
    });
    enriched.ocr_observations.push(SampledOcr {
        scene_id: "scene-0".into(),
        frame,
        observation: RasterObservation {
            provider: "ocr".into(),
            model_revision: None,
            coordinate_space: "frame_pixels_top_left".into(),
            regions: vec![RasterRegion {
                text: "Title\n=====".into(),
                left: 0.,
                top: 0.,
                right: 1.,
                bottom: 1.,
                geometry_level: "ocr_region".into(),
            }],
            truncated: false,
        },
    });
    enriched.timeline.cues[0].text = "# cue heading".into();
    assert_eq!(node_ids(&metadata_only), node_ids(&enriched));
}
