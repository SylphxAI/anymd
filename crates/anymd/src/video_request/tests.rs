use super::cache::cacheable;
use super::sidecars::{apply_transcript_cues, attach_sidecars, load_sidecar, sidecar_text};
use super::{inspect, TimelineSelection};
use crate::schema::InspectArgs;
use crate::source_access::SourceAccessPolicy;
use anymd_formats::video::timeline;
use serde_json::json;

#[test]
fn operation_options_fail_before_source_or_worker_work() {
    for value in [
        json!({"operation":"video_timeline","sources":[{"path":"missing.mkv"}],"timeline":{"end_ms":0}}),
        json!({"operation":"video_timeline","sources":[{"path":"missing.mkv"}],"timeline":{"end_ms":1000},"include_image":true}),
        json!({"operation":"render_frame","sources":[{"path":"missing.mkv"}],"timestamps_ms":[]}),
        json!({"operation":"render_frame","sources":[{"path":"missing.mkv"}],"timestamps_ms":[0],"ocr":"vlm"}),
        json!({"operation":"render_frame","sources":[{"path":"missing.mkv","pages":"1"}],"timestamps_ms":[0]}),
    ] {
        let args: InspectArgs = serde_json::from_value(value).unwrap();
        let error = inspect(args, &SourceAccessPolicy::unrestricted()).unwrap_err();
        assert!(!error.message.contains("No such file"), "{:?}", error);
    }
}
#[test]
fn malformed_sidecar_is_a_subtitles_gap_not_a_request_error() {
    let mut evidence = crate::video_evidence::tests::fixture();
    evidence.timeline.cues.clear();
    evidence.timeline.components.subtitles = timeline::Component::not_requested();
    attach_sidecars(
        &mut evidence.timeline,
        &[
            (
                "srt".into(),
                Ok("1\n00:00:01,000 --> 00:00:01,000\nzero".into()),
            ),
            (
                "vtt".into(),
                Ok("WEBVTT\n\n00:00.100 --> 00:00.200\nok".into()),
            ),
        ],
    );
    let subtitles = &evidence.timeline.components.subtitles;
    assert_eq!(subtitles.status, timeline::Status::Partial);
    assert!(subtitles
        .reason
        .as_deref()
        .unwrap()
        .contains("subtitle file srt"));
    assert_eq!(evidence.timeline.cues.len(), 1);
    evidence.timeline.components.subtitles = timeline::Component::gap("embedded unavailable");
    attach_sidecars(
        &mut evidence.timeline,
        &[(
            "vtt".into(),
            Ok("WEBVTT\n\n00:00.300 --> 00:00.400\nok".into()),
        )],
    );
    assert_eq!(
        evidence.timeline.components.subtitles.status,
        timeline::Status::Unavailable
    );
}
#[test]
fn unreadable_sidecars_are_gaps_and_the_request_survives() {
    let dir = tempfile::tempdir().unwrap();
    let policy = SourceAccessPolicy::unrestricted();
    let latin1 = dir.path().join("movie.srt");
    std::fs::write(&latin1, b"1\n00:00:01,000 --> 00:00:02,000\ncaf\xe9\n").unwrap();
    let reason = load_sidecar(&latin1, &policy).unwrap_err();
    assert!(reason.contains("UTF-8"), "{reason}");
    let big = dir.path().join("big.srt");
    std::fs::write(&big, vec![b'a'; 4 * 1024 * 1024 + 1]).unwrap();
    assert!(load_sidecar(&big, &policy).unwrap_err().contains("4 MiB"));
    assert!(load_sidecar(dir.path(), &policy).is_err());
    let mut evidence = crate::video_evidence::tests::fixture();
    evidence.timeline.cues.clear();
    evidence.timeline.components.subtitles = timeline::Component::not_requested();
    attach_sidecars(
        &mut evidence.timeline,
        &[
            ("srt".into(), Err(reason)),
            ("vtt".into(), Err("is a directory".into())),
        ],
    );
    let subtitles = &evidence.timeline.components.subtitles;
    assert_eq!(subtitles.status, timeline::Status::Partial);
    let text = subtitles.reason.as_deref().unwrap();
    assert!(text.contains("subtitle file srt") && text.contains("subtitle file vtt"));
}
#[test]
fn a_sidecar_outside_the_allowlist_is_refused() {
    let cwd = tempfile::tempdir().unwrap();
    let allowed = tempfile::tempdir().unwrap();
    let allowed_media = allowed.path().join("movie.mp4");
    std::fs::write(&allowed_media, b"not a video").unwrap();
    std::fs::write(allowed.path().join("movie.srt"), "WEBVTT\n").unwrap();
    let policy =
        SourceAccessPolicy::restricted_for_test(cwd.path().to_path_buf(), allowed.path()).unwrap();
    let admitted = load_sidecar(&allowed_media.with_extension("srt"), &policy);
    assert_eq!(admitted.unwrap(), "WEBVTT\n");
    // A sibling outside the allowlist stays unreadable, admitted or not.
    let outside = cwd.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("movie.srt"), "WEBVTT\n").unwrap();
    let probe = sidecar_text(&outside.join("movie.mp4"), "srt", &policy);
    assert!(probe.unwrap_err().contains("Access denied"));
    let direct = load_sidecar(&outside.join("movie.srt"), &policy);
    assert!(direct.unwrap_err().contains("Access denied"));
}
#[test]
fn sidecar_errors_keep_the_worse_status_and_earlier_reasons() {
    let mut evidence = crate::video_evidence::tests::fixture();
    evidence.timeline.components.subtitles = timeline::Component::gap("embedded unavailable");
    attach_sidecars(&mut evidence.timeline, &[("srt".into(), Err("bad".into()))]);
    let subtitles = &evidence.timeline.components.subtitles;
    assert_eq!(subtitles.status, timeline::Status::Unavailable);
    let text = subtitles.reason.as_deref().unwrap();
    assert!(text.contains("embedded unavailable") && text.contains("subtitle file srt: bad"));
}
#[test]
fn unknown_origin_with_sidecar_cues_reports_what_is_attached() {
    let mut evidence = crate::video_evidence::tests::fixture();
    evidence.timeline.cues.clear();
    evidence.timeline.clock.as_mut().unwrap().origin_pts_us = None;
    evidence.timeline.components.subtitles = timeline::Component::gap(
        "media origin unavailable; embedded subtitle cues cannot be aligned",
    );
    attach_sidecars(
        &mut evidence.timeline,
        &[(
            "vtt".into(),
            Ok("WEBVTT\n\n00:00.300 --> 00:00.400\nok".into()),
        )],
    );
    let subtitles = &evidence.timeline.components.subtitles;
    assert_eq!(subtitles.status, timeline::Status::Partial);
    let text = subtitles.reason.as_deref().unwrap();
    assert!(
        text.contains("embedded") && text.contains("own playback clock"),
        "{text}"
    );
    assert!(!evidence.timeline.cues.is_empty());
}
#[test]
fn invalid_transcript_cue_timing_is_a_gap_not_an_error() {
    let mut evidence = crate::video_evidence::tests::fixture();
    let bad = timeline::Cue {
        track: "asr".into(),
        start_ms: 5,
        end_ms: 5,
        text: "x".into(),
        timing: "segment".into(),
        provider: "t".into(),
    };
    assert!(timeline::attach_cues(&mut evidence.timeline, vec![bad.clone()], true).is_err());
    apply_transcript_cues(&mut evidence.timeline, vec![bad]);
    assert_eq!(
        evidence.timeline.components.transcript.status,
        timeline::Status::Unavailable
    );
}
#[test]
fn partial_subtitles_or_chapters_are_not_cacheable() {
    let mut evidence = crate::video_evidence::tests::fixture();
    assert!(cacheable(&evidence));
    evidence.timeline.components.subtitles = timeline::Component::partial("deadline");
    assert!(!cacheable(&evidence));
    evidence.timeline.components.subtitles = timeline::Component::ok();
    evidence.timeline.components.chapters = timeline::Component::partial("deadline");
    assert!(!cacheable(&evidence));
}
#[test]
fn additive_options_preserve_legacy_and_require_end() {
    assert!(serde_json::from_value::<TimelineSelection>(json!({"start_ms":0})).is_err());
    let selection: TimelineSelection = serde_json::from_value(json!({"end_ms":1000})).unwrap();
    assert_eq!(selection.max_scenes, 20);
    assert!(!selection.caption);
    let old: crate::schema::ReadArgs =
        serde_json::from_value(json!({"source":"x.pdf","ocr":true})).unwrap();
    assert!(old.timeline.is_none());
    assert!(old.ocr.unwrap().enabled());
    let old: crate::schema::OutlineArgs =
        serde_json::from_value(json!({"source":"x.pdf"})).unwrap();
    assert!(old.timeline.is_none());
}
