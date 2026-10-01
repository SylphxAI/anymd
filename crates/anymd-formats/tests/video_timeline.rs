//! Real media acceptance, explicitly CI-only. No models or private media.
//! Run on a disposable runner with CI=true:
//! cargo test -p anymd-formats --test video_timeline -- --ignored
#![cfg(feature = "native")]
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anymd_formats::video::timeline::{self, Status, SubtitleTrack, TimelineOptions};
use sha2::{Digest, Sha256};

fn runner() {
    assert_eq!(
        std::env::var("CI").as_deref(),
        Ok("true"),
        "real FFmpeg tests belong on a disposable CI runner only"
    );
}
fn fixture(dir: &Path, variable: bool) -> PathBuf {
    runner();
    let path = dir.join(if variable { "vfr.mkv" } else { "cuts.mkv" });
    let subtitles = dir.join("overlap.srt");
    std::fs::write(&subtitles, "1\n00:00:00,400 --> 00:00:01,537\ncross-cut\n\n2\n00:00:00,900 --> 00:00:01,900\noverlap\n").unwrap();
    // Nonzero origin, audio offset, one hard cut and VFR intervals. Lossless
    // tiny frames avoid codec/keyframe approximation in the expected timestamps.
    let select = if variable {
        ",select='eq(mod(n,3),0)+eq(mod(n,7),0)'"
    } else {
        ""
    };
    let filter = format!(
        "[0:v][1:v]concat=n=2:v=1:a=0{select},setpts=PTS+5/TB[v];[2:a]asetpts=PTS+5.2/TB[a]"
    );
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x48:r=20:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=c=white:s=64x48:r=20:d=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=16000:duration=1.8",
            "-itsoffset",
            "5",
            "-i",
            subtitles.to_str().unwrap(),
            "-filter_complex",
            &filter,
            "-map",
            "[v]",
            "-map",
            "[a]",
            "-map",
            "3:0",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
            "-c:s",
            "srt",
            "-copyts",
            "-fps_mode",
            "passthrough",
            path.to_str().unwrap(),
        ])
        .output()
        .expect("CI installs ffmpeg");
    assert!(
        output.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    path
}
fn hash(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}
fn options() -> TimelineOptions {
    TimelineOptions {
        start_ms: 0,
        end_ms: 2000,
        max_scenes: 20,
        caption: false,
    }
}

#[test]
#[ignore = "real FFmpeg on disposable CI runner only"]
fn hard_cut_nonzero_pts_audio_offset_overlap_and_real_frame_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), false);
    let deadline = Instant::now() + Duration::from_secs(30);
    let timeline = timeline::extract(&path, &hash(&path), &options(), deadline).unwrap();
    assert_eq!(timeline.components.scenes.status, Status::Ok);
    assert_eq!(timeline.scenes.len(), 2, "{timeline:?}");
    assert_eq!(timeline.scenes[0].start_ms, 0);
    assert_eq!(timeline.scenes[1].start_ms, 1000);
    assert_eq!(timeline.scenes[1].detection, "detected_cut");
    let clock = timeline.clock.as_ref().unwrap();
    assert_eq!(clock.origin_pts_us, Some(5_000_000));
    assert_eq!(clock.audio_start_pts_us, Some(5_200_000));
    let cue = timeline
        .cues
        .iter()
        .find(|c| c.text.contains("cross-cut"))
        .unwrap();
    assert_eq!((cue.start_ms, cue.end_ms), (400, 1537));
    let frame = timeline::render_frame(&path, clock, 1031, 2000, deadline)
        .unwrap()
        .unwrap();
    assert_eq!(frame.metadata.requested_ms, 1031);
    assert_eq!(frame.metadata.actual_ms, 1050);
    assert_eq!(frame.metadata.decoded_pts_us, 6_050_000);
    assert_eq!((frame.metadata.width, frame.metadata.height), (64, 48));
    assert_eq!(
        frame.metadata.sha256,
        format!("{:x}", Sha256::digest(&frame.png))
    );
    // A keyframe timestamp or simply echoing 1031 cannot satisfy this check.
    assert_ne!(frame.metadata.actual_ms, frame.metadata.requested_ms as i64);
}

#[test]
#[ignore = "real FFmpeg on disposable CI runner only"]
fn vfr_frame_is_first_decodable_after_request_not_nominal_fps() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), true);
    let deadline = Instant::now() + Duration::from_secs(30);
    let timeline = timeline::extract(&path, &hash(&path), &options(), deadline).unwrap();
    let clock = timeline.clock.as_ref().unwrap();
    // Original n=0,3,6,7 => 0,150,300,350 ms. There is no 100ms frame.
    let frame = timeline::render_frame(&path, clock, 101, 2000, deadline)
        .unwrap()
        .unwrap();
    assert_eq!(frame.metadata.actual_ms, 150);
    assert_eq!(frame.metadata.decoded_pts_us, 5_150_000);
}

#[test]
#[ignore = "real FFmpeg on disposable CI runner only"]
fn continuous_scene_bounded_window_and_no_frame_after_media_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), false);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut selected = options();
    selected.start_ms = 200;
    selected.end_ms = 900;
    let timeline = timeline::extract(&path, &hash(&path), &selected, deadline).unwrap();
    assert_eq!(timeline.scenes.len(), 1);
    assert_eq!(timeline.scenes[0].start_ms, 200);
    assert_eq!(timeline.scenes[0].end_ms, 900);
    let clock = timeline.clock.as_ref().unwrap();
    assert!(
        timeline::render_frame(&path, clock, 2500, 3000, deadline)
            .unwrap()
            .is_none()
    );
    assert!(timeline::render_frame(&path, clock, 0, 0, deadline).is_err());
    assert!(
        timeline::extract(
            &path,
            &hash(&path),
            &selected,
            Instant::now() - Duration::from_secs(1)
        )
        .is_err()
    );
    let sidecar = SubtitleTrack {
        id: "sidecar:en",
        playback_offset_ms: 0,
        text: "WEBVTT\n\n00:00.400 --> 00:01.537\ncross-cut\n\n00:00.900 --> 00:01.900\noverlap",
    };
    let cues = timeline::subtitle_cues(&sidecar).unwrap();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].end_ms, 1537);
}
