"""Lightweight source contracts only; never execute Rust, FFmpeg or providers.

These checks do not replace compilation, Rust tests, real-media CI or quality
acceptance. They guard the narrow integration/ownership constraints on the desk.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
BACKEND = (ROOT / "crates/anymd-formats/src/video/timeline.rs").read_text()
VIDEO = (ROOT / "crates/anymd-formats/src/video.rs").read_text()
RUNNER = (ROOT / "crates/anymd-formats/src/tool.rs").read_text()
EVIDENCE = (ROOT / "crates/anymd/src/video_evidence.rs").read_text()
NATIVE = (ROOT / "crates/anymd-formats/tests/video_timeline.rs").read_text()


class VideoSourceContracts(unittest.TestCase):
    def test_integer_half_open_window_and_caps(self):
        for contract in ("pub start_ms: u64", "pub end_ms: u64", "MAX_WINDOW_MS: u64 = 600_000",
                         "MAX_SCENES: usize = 20", "MAX_FRAME_PIXELS: u64 = 2_000_000",
                         "MAX_FRAME_BYTES: u64 = 4 * 1024 * 1024", "self.end_ms <= self.start_ms"):
            self.assertIn(contract, BACKEND)

    def test_decoded_pts_not_seek_echo_or_keyframe(self):
        self.assertIn("decoded_time_base", BACKEND)
        self.assertIn("exact_pts_us(ticks, &time_base)", BACKEND)
        self.assertIn("decoded_pts: ticks", BACKEND)
        self.assertIn("Sha256::digest(&output.stdout)", BACKEND)
        self.assertNotIn("setpts=", BACKEND)
        self.assertNotIn("skip_frame", BACKEND)

    def test_existing_runner_only_and_lower_caps_fail_closed(self):
        self.assertIn("tool::run_bounded", BACKEND)
        self.assertNotIn("Command::new", BACKEND)
        self.assertNotIn("Command::new", EVIDENCE)
        self.assertIn("maximum.saturating_add(1)", RUNNER)
        self.assertIn("exceeded the operation output budget", RUNNER)
        self.assertIn("remaining(deadline)?", BACKEND)

    def test_no_source_admission_or_sidecar_or_model_duplicate(self):
        for prohibited in ("fetch_url", "read_dir", "ensure_model", "Semaphore", "ACTIVE_REQUESTS"):
            self.assertNotIn(prohibited, BACKEND)
            self.assertNotIn(prohibited, EVIDENCE)
        structured = VIDEO[VIDEO.index("pub fn transcript_window("):VIDEO.index("fn transcribe_structured(")]
        self.assertIn("transcribe_structured(path, false, Some(&window))", structured)
        self.assertIn("existing isolated request worker", VIDEO)

    def test_endpoint_overlap_provenance_and_initial_cut_policy(self):
        self.assertIn("pub end_ms: i64", BACKEND)
        self.assertIn("X-TIMESTAMP-MAP", BACKEND)
        self.assertIn("ffmpeg_detected_cuts_v1_threshold_0.4", BACKEND)
        self.assertIn("initial_window_scene", BACKEND)
        self.assertIn("eq(n", BACKEND)
        self.assertIn("scene", BACKEND)
        self.assertIn("0.4", BACKEND)
        self.assertIn("cue.end_ms", EVIDENCE)
        self.assertIn("timeline cues", EVIDENCE)

    def test_borrowed_budget_local_caption_cache_and_no_recaption_frame_route(self):
        self.assertIn("pub trait FrameContext", EVIDENCE)
        self.assertIn("context.charge_frame", EVIDENCE)
        self.assertIn("context.local_caption_available()", EVIDENCE)
        self.assertIn('description.provider != "external-command"', EVIDENCE)
        frame_route = EVIDENCE[EVIDENCE.index("pub fn render_frames("):EVIDENCE.index("pub fn sections(")]
        self.assertNotIn("caption_cached(", frame_route)
        self.assertNotIn("context.ocr(", frame_route)
        self.assertIn("timestamps_ms.len() > 20", frame_route)

    def test_real_fixture_is_ci_only_and_ignored_until_requested(self):
        self.assertEqual(NATIVE.count('#[ignore = "real FFmpeg on disposable CI runner only"]'), 3)
        self.assertIn('std::env::var("CI")', NATIVE)
        self.assertIn("vfr_frame_is_first_decodable", NATIVE)
        self.assertIn("hard_cut_nonzero_pts_audio_offset_overlap", NATIVE)
        self.assertIn("fixture generation failed", NATIVE)

    def test_legacy_default_path_remains_separate(self):
        self.assertIn("transcribe_structured(path, download, None)", VIDEO)
        self.assertIn("if pcm.len() < SAMPLES_PER_CHUNK && window.is_none()", VIDEO)
        self.assertIn('pub mod timeline;', VIDEO)
        self.assertIn("run_captured(program, args, env, timeout, MAX_OUTPUT, MAX_OUTPUT, false)", RUNNER)


if __name__ == "__main__":
    unittest.main()
