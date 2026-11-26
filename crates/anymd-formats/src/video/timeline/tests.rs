use super::probe::exact_pts_us;
use super::*;
use serde_json::json;
fn clock() -> MediaClock {
    metadata(
        &json!({"format":{"start_time":"5.0", "duration":"3.0"},"streams":[
        {"index":0,"codec_type":"video","start_time":"5.2","time_base":"1/1000"},
        {"index":1,"codec_type":"audio","start_time":"5.0"}]}),
    )
    .unwrap()
    .0
}
#[test]
fn timestamps_offsets_vfr_and_initial_scene() {
    let c = clock();
    assert_eq!(c.video_start_pts_us, Some(5_200_000));
    assert_eq!(c.audio_start_pts_us, Some(5_000_000));
    let pts = decoded_pts("[Parsed_showinfo_2] n: 0 pts: 5200 pts_time:5.2\n[Parsed_showinfo_2] n: 1 pts: 5537 pts_time:5.537").unwrap();
    assert_eq!(c.playback_us(pts[1].1), Some(537_000));
    let (scenes, partial) = scenes_from_pts(
        &c,
        &TimeRange {
            start_ms: 0,
            end_ms: 3000,
        },
        &pts,
        2,
    );
    assert!(partial);
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[0].start_ms, 0);
    assert_eq!(scenes[1].end_ms, 3000);
}
#[test]
fn subtitles_keep_end_overlap_repeat_and_cross_cut() {
    let track = SubtitleTrack {
        id: "english",
        playback_offset_ms: 200,
        text: "WEBVTT\n\n00:00.100 --> 00:01.537\nrepeat\n\n00:00.900 --> 00:02.100\nrepeat",
    };
    let cues = subtitle_cues(&track).unwrap();
    assert_eq!((cues[0].start_ms, cues[0].end_ms), (300, 1737));
    assert_eq!(cues[0].text, cues[1].text);
    assert!(cues[1].start_ms < cues[0].end_ms);
}
#[test]
fn malformed_sidecars_error_without_hiding_prior_gaps() {
    for text in [
        "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,MPEGTS:450000\n\n00:00.100 --> 00:00.200\nx",
        "1\n00:00:01,000 --> 00:00:01,000\nzero length",
    ] {
        let track = SubtitleTrack {
            id: "side",
            playback_offset_ms: 0,
            text,
        };
        assert!(subtitle_cues(&track).is_err());
    }
}
#[test]
fn unknown_origin_is_not_zero() {
    let (clock, _, _) = metadata(
        &json!({"format":{}, "streams":[{"index":0,"codec_type":"video","time_base":"1/90"}]}),
    )
    .unwrap();
    assert!(clock.playback_us(0).is_none());
}
#[test]
fn exact_filter_timebase_and_unresolved_subtitle_map() {
    let log = "[Parsed_showinfo_2] config in time_base: 1/90000, frame_rate: 25/1";
    assert_eq!(decoded_time_base(log).unwrap(), "1/90000");
    assert_eq!(exact_pts_us(465300, "1/90000").unwrap(), 5_170_000);
    assert!(exact_pts_us(1, "1/0").is_err());
    assert!(decoded_time_base("pts_time:5.17").is_err());
    let track = SubtitleTrack {
        id: "mapped",
        playback_offset_ms: 0,
        text:
            "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,MPEGTS:450000\n\n00:00.100 --> 00:00.200\ntext",
    };
    assert!(subtitle_cues(&track).is_err());
}
#[test]
fn strict_window_and_scene_limits() {
    let mut options = TimelineOptions {
        start_ms: 0,
        end_ms: 600_000,
        max_scenes: 20,
        caption: false,
    };
    assert!(options.validate().is_ok());
    options.end_ms += 1;
    assert!(options.validate().is_err());
    options.end_ms = 1;
    options.max_scenes = 0;
    assert!(options.validate().is_err());
    assert!(serde_json::from_str::<TimelineOptions>("{\"start_ms\":0}").is_err());
}
