use std::{path::Path, time::Instant};

use sha2::{Digest, Sha256};

use super::probe::{args, decoded_pts, decoded_time_base, exact_pts_us, remaining, seconds};
use super::types::{
    DecodedFrame, FrameMetadata, MediaClock, LOG_BYTES, MAX_FRAME_BYTES, MAX_FRAME_PIXELS,
    MAX_WINDOW_MS,
};
use crate::tool;

/// First decodable frame >= requested playback timestamp, within a caller-bound
/// half-open interval. No seek approximation, PTS reset, captioning or caching.
pub fn render_frame(
    path: &Path,
    clock: &MediaClock,
    requested_ms: u64,
    end_ms: u64,
    deadline: Instant,
) -> Result<Option<DecodedFrame>, String> {
    if end_ms <= requested_ms || end_ms - requested_ms > MAX_WINDOW_MS {
        return Err("frame interval must be positive and at most ten minutes".into());
    }
    let ffmpeg = tool::find("ffmpeg").ok_or("ffmpeg unavailable")?;
    let start = seconds(clock.absolute_us(requested_ms)?);
    let end = seconds(clock.absolute_us(end_ms)?);
    // Bound even pathological portrait/landscape sources before PNG encoding.
    let filter = format!(
        "trim=start={start}:end={end},scale=w='min(iw,1414)':h='min(ih,1414)':force_original_aspect_ratio=decrease,showinfo"
    );
    let mut ffargs = args(&[
        "-nostdin",
        "-v",
        "info",
        "-protocol_whitelist",
        "file,pipe",
        "-copyts",
        "-i",
    ]);
    ffargs.push(path.as_os_str().into());
    ffargs.extend(args(&[
        "-map",
        &format!("0:{}", clock.video_stream),
        "-an",
        "-sn",
        "-vf",
        &filter,
        "-frames:v",
        "1",
        "-fps_mode",
        "passthrough",
        "-c:v",
        "png",
        "-f",
        "image2pipe",
        "pipe:1",
    ]));
    let output = tool::run_bounded(
        &ffmpeg,
        ffargs,
        remaining(deadline)?,
        MAX_FRAME_BYTES,
        LOG_BYTES,
    )?;
    if !output.success {
        return Err("FFmpeg frame decode failed".into());
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    let pts = decoded_pts(&String::from_utf8_lossy(&output.stderr))?;
    // showinfo can observe a lookahead frame. The first record belongs to the
    // one encoded PNG; never choose a later timestamp from the log.
    let (ticks, _) = *pts.first().ok_or("decoded PNG has no timestamp evidence")?;
    let time_base = decoded_time_base(&String::from_utf8_lossy(&output.stderr))?;
    let pts_us = exact_pts_us(ticks, &time_base)?;
    let playback_us = clock
        .playback_us(pts_us)
        .ok_or("frame origin unavailable")?;
    if playback_us < i64::try_from(requested_ms).map_err(|_| "time overflow")? * 1000
        || playback_us >= i64::try_from(end_ms).map_err(|_| "time overflow")? * 1000
    {
        return Err("decoded frame outside requested interval".into());
    }
    let size = imagesize::blob_size(&output.stdout).map_err(|_| "invalid decoded PNG")?;
    if size.width == 0
        || size.height == 0
        || (size.width as u64) * (size.height as u64) > MAX_FRAME_PIXELS
    {
        return Err("decoded frame exceeds pixel budget".into());
    }
    let sha256 = hex::encode(Sha256::digest(&output.stdout));
    Ok(Some(DecodedFrame {
        metadata: FrameMetadata {
            requested_ms,
            actual_ms: playback_us / 1000,
            decoded_pts: ticks,
            decoded_pts_us: pts_us,
            stream: clock.video_stream,
            time_base,
            width: size.width as u32,
            height: size.height as u32,
            sha256,
        },
        png: output.stdout,
    }))
}
