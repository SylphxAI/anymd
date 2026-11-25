use std::time::Duration;

use super::subtitles::parse_timestamp;
use super::transcribe::{parse_alignment, render_transcript, TranscriptCue};
use super::{container, convert, sniff, subtitles_to_markdown};
use crate::{tool, ConvertError, Options};

#[test]
fn sniffs_containers() {
    assert!(sniff(b"\0\0\0\x20ftypisom\0\0\0\0"));
    assert!(!sniff(b"\0\0\0\x20ftypavif\0\0\0\0"));
    assert!(sniff(&[
        0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x82, 0x84, b'w', b'e', b'b', b'm'
    ]));
    assert_eq!(container(b"RIFF\0\0\0\0WAVEfmt ").map(|c| c.0), Some("WAV"));
    assert_eq!(container(b"RIFF\0\0\0\0AVI LIST").map(|c| c.0), Some("AVI"));
    assert!(sniff(b"ID3\x04\0\0\0\0"));
    assert!(sniff(&[0xFF, 0xFB, 0x90, 0x64]));
    assert!(sniff(b"fLaC\0\0\0\x22"));
    assert!(sniff(b"OggS\0\x02"));
    assert!(!sniff(b"plain text"));
    assert!(!sniff(&[0xFF, 0xD8, 0xFF, 0xE0]));
}

#[test]
fn srt_to_timestamped_lines() {
    let srt = "\u{feff}1\r\n00:00:01,500 --> 00:00:03,000\r\n<i>Hello</i>\r\nthere\r\n\r\n2\r\n00:01:02,000 --> 00:01:04,000\r\n{\\an8}General &amp; Kenobi\r\n\r\n";
    assert_eq!(
        subtitles_to_markdown(srt),
        "[00:01.500] Hello there\n[01:02] General & Kenobi"
    );
}

#[test]
fn vtt_voices_repeated_captions_and_hours() {
    let vtt = "WEBVTT\nKind: captions\n\nNOTE a comment\n\n00:00.000 --> 00:02.000\n<v Roger Bingham>We are here\n\n\
               00:02.000 --> 00:04.000 align:start\nwe are here\nand now more\n\n\
               00:04.000 --> 00:06.000\nand now more\n\n01:00:00.000 --> 01:00:01.000\n<c.yellow>late</c>";
    assert_eq!(
        subtitles_to_markdown(vtt),
        "[00:00:00] Roger Bingham: We are here\n[00:00:02] we are here and now more\n[00:00:04] and now more\n[01:00:00] late"
    );
}

#[test]
fn every_authored_cue_and_line_survives_at_its_source_start() {
    // The first two cues overlap; later identical text is still evidence.
    let srt = "1\n00:00:00,125 --> 00:00:02,000\n<i>Again</i>\n\n\
               2\n00:00:01,250 --> 00:00:03,000\nAgain\nAgain\nnew line\n\n\
               3\n00:00:04,005 --> 00:00:05,000\nAgain\n\n\
               4\n00:00:06,000 --> 00:00:07,000\n<c>你好，世界</c>\n\n\
               5\n00:00:07,000 --> 00:00:08,000\n你好，世界\n\n\
               6\n00:00:07,000 --> 00:00:09,000\n你好，世界\n";
    assert_eq!(subtitles_to_markdown(srt), "[00:00.125] Again\n[00:01.250] Again Again new line\n[00:04.005] Again\n[00:06] 你好，世界\n[00:07] 你好，世界\n[00:07] 你好，世界");
}

#[test]
fn vtt_preserves_voice_markup_cleanup_and_repeated_multiline_text() {
    let vtt = "WEBVTT\n\nfirst\n00:00.100 --> 00:02.000\n<v.speaker Alice><b>Hi &amp; bye</b></v>\n你好\n\n\
               second\n00:01.200 --> 00:03.000 align:start\n<v Alice>Hi &amp; bye</v>\n你好\n你好\n\n\
               third\n01:02:03.456 --> 01:02:04.000\n{\\an8}<i>late &lt;cue&gt;</i>\n";
    assert_eq!(subtitles_to_markdown(vtt), "[00:00:00.100] Alice: Hi & bye 你好\n[00:00:01.200] Alice: Hi & bye 你好 你好\n[01:02:03.456] late <cue>");
}

#[test]
fn empty_timed_cues_do_not_fall_back_to_subtitle_syntax() {
    assert_eq!(
        subtitles_to_markdown("1\n00:00:00,000 --> 00:00:01,000\n<i></i>\n"),
        ""
    );
    assert_eq!(
        subtitles_to_markdown("  ordinary text\r\nnext line  "),
        "ordinary text\nnext line"
    );
}

#[test]
fn whitespace_separators_and_vtt_comments_do_not_add_or_drop_cues() {
    let vtt = "WEBVTT\n\nNOTE not a cue\n00:09.000 --> 00:10.000\ncomment text\n\n\
               00:00.000 --> 00:02.000\nsame\n \t\n\
               00:01.000 --> 00:03.000\nsame\n";
    assert_eq!(subtitles_to_markdown(vtt), "[00:00] same\n[00:01] same");
}

#[test]
fn cue_source_order_is_not_sorted_or_merged() {
    let srt = "1\n00:00:02,000 --> 00:00:04,000\nsame\n\n\
               2\n00:00:01,000 --> 00:00:03,000\nsame\n\n\
               3\n00:00:01,000 --> 00:00:03,000\nsame\n";
    assert_eq!(
        subtitles_to_markdown(srt),
        "[00:02] same\n[00:01] same\n[00:01] same"
    );
}

#[test]
fn alignment_preserves_text_and_validates_bounds() {
    let json = include_str!("../../tests/fixtures/aligner/crispasr-0.8.38-words.json");
    let cues = parse_alignment(json, "Hello world", 1000).unwrap();
    assert_eq!(cues[0].start_ms, 100);
    assert!(render_transcript(&cues).contains("word (Qwen3-ForcedAligner)"));
    assert!(parse_alignment(json, "Hello world again", 1000).is_err());
    assert!(parse_alignment(json, "Hello world", 900).is_err());
    assert!(parse_alignment("[]", "Hello", 1000).is_err());
    assert!(parse_alignment(r#"[{"start":1.0,"end":0.5,"word":"Hello"}]"#, "Hello", 1000).is_err());
    assert!(parse_alignment(
        r#"[{"start":0.1,"end":0.7,"word":"Hello"},{"start":0.6,"end":0.9,"word":"world"}]"#,
        "Hello world",
        1000
    )
    .is_err());
}

#[test]
fn chunk_timestamps_remain_on_the_source_timeline() {
    let cues = vec![
        TranscriptCue {
            start_ms: 20_000,
            end_ms: 40_000,
            text: "second chunk".into(),
            word: false,
        },
        TranscriptCue {
            start_ms: 40_000,
            end_ms: 41_250,
            text: "last partial chunk".into(),
            word: false,
        },
    ];
    let rendered = render_transcript(&cues);
    assert!(rendered.contains("segment (20-second chunk boundaries, not word timing)"));
    assert!(rendered.contains("[00:00:20.000 --> 00:00:40.000] second chunk"));
    assert!(rendered.contains("[00:00:40.000 --> 00:00:41.250] last partial chunk"));
    assert!(render_transcript(&[]).is_empty());
}

#[test]
fn timestamps_parse_and_reject_garbage() {
    assert_eq!(parse_timestamp("01:02:03,004"), Some(3_723_004));
    assert_eq!(parse_timestamp("02:03.5"), Some(123_500));
    assert_eq!(parse_timestamp("nope"), None);
    assert_eq!(parse_timestamp("99999999999999999999:00:00"), None);
    assert_eq!(subtitles_to_markdown("just text"), "just text");
}

#[test]
fn unrecognized_bytes_are_invalid() {
    assert!(matches!(
        convert(b"definitely not media", &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
}

/// End-to-end with real ffmpeg when present: a 1 s clip with a chapter and subtitles.
#[test]
fn probes_generated_clip_when_ffmpeg_exists() {
    let Some(ffmpeg) = tool::find("ffmpeg") else {
        return;
    };
    if tool::find("ffprobe").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let srt = dir.path().join("in.srt");
    std::fs::write(&srt, "1\n00:00:00,100 --> 00:00:00,900\nHi there\n").unwrap();
    let meta = dir.path().join("meta.txt");
    std::fs::write(
        &meta,
        ";FFMETADATA1\ntitle=Clip\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=Opening\n",
    )
    .unwrap();
    let clip = dir.path().join("clip.mkv");
    let sidecar = dir.path().join("clip.en.srt");
    std::fs::write(&sidecar, "1\n00:00:00,000 --> 00:00:01,000\nSidecar line\n").unwrap();
    let args: Vec<std::ffi::OsString> = [
        "-nostdin",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=d=1:s=64x48:r=10",
        "-f",
        "lavfi",
        "-i",
        "sine=d=1",
        "-i",
    ]
    .iter()
    .map(Into::into)
    .chain([srt.into_os_string(), "-i".into(), meta.into_os_string()])
    .chain(
        [
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:s",
            "-map_metadata",
            "3",
            "-map_chapters",
            "3",
            "-c:v",
            "mpeg4",
            "-c:a",
            "flac",
            "-c:s",
            "srt",
            "-y",
        ]
        .iter()
        .map(Into::into),
    )
    .chain([clip.clone().into_os_string()])
    .collect();
    let made = tool::run(&ffmpeg, &args, Duration::from_secs(60)).unwrap();
    if !made.success {
        return; // ffmpeg build without these codecs; nothing to assert.
    }
    let bytes = std::fs::read(&clip).unwrap();
    let converted = convert(
        &bytes,
        &Options {
            path: Some(clip.clone()),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(converted.title.as_deref(), Some("Clip"));
    let media = &converted.sections[0].markdown;
    assert!(media.contains("|Kind|video|"), "{media}");
    assert!(media.contains("- Video: mpeg4, 64×48, 10 fps"), "{media}");
    assert!(media.contains("- Audio: flac, 44.1 kHz"), "{media}");
    assert!(media.contains("## Chapters\n\n- 00:00 Opening"), "{media}");
    assert!(media.contains("`transcript: true`"), "{media}");
    let labels: Vec<&str> = converted
        .sections
        .iter()
        .map(|s| s.label.as_str())
        .collect();
    assert_eq!(labels, ["media", "subtitles", "subtitles (clip.en.srt)"]);
    assert_eq!(converted.sections[1].markdown, "[00:00.100] Hi there");
    assert_eq!(converted.sections[2].markdown, "[00:00] Sidecar line");

    // Bytes only (no path): probed through a temp file.
    let converted = convert(&bytes, &Options::default()).unwrap();
    assert_eq!(converted.sections.len(), 2);

    // The transcript path downloads weights only when explicitly requested;
    // ordinary media conversion above remains offline and model-free.
}
