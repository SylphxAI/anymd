# Video timelines and frames

Use `inspect` for a bounded, ordered video timeline or an on-demand decoded
frame. `read` projects the same timeline as document sections; `outline`
navigates its stable scene and chapter headings. These remain operations of the
existing four tools, not separate tools.

## Timeline

```json
{
  "operation": "video_timeline",
  "sources": [{ "path": "/media/demo.mkv" }],
  "timeline": { "start_ms": 0, "end_ms": 120000, "max_scenes": 20, "caption": false }
}
```

`end_ms` is required. The interval is half-open, positive and at most ten
minutes. `max_scenes` defaults to 20 and cannot exceed 20. The timeline reports
its source hash, requested/processed range and mapping from container PTS to
playback milliseconds. Unknown clock alignment remains unavailable, not zero.

Scenes include the initial decoded scene and FFmpeg **detected cuts**, using
policy `ffmpeg_detected_cuts_v1_threshold_0.4`. They are heuristic cuts, not
verified semantic scenes. Excess cuts are coalesced into the final scene with
partial detection status; chapters and codec keyframes are not substitutes.

Structured subtitle/ASR cues retain both endpoints, overlapping/cross-cut cues,
track identity and timing granularity. Repeated subtitle text remains in the
structured evidence. Forced-aligned words and 20-second ASR segments are not
interchangeable. Sidecars are admitted separately; unresolved WebVTT timestamp
maps are explicit gaps. There is no model download in timeline extraction.

## Frames

```json
{
  "operation": "render_frame",
  "sources": [{ "path": "/media/demo.mkv" }],
  "timestamps_ms": [1031, 1500],
  "include_image": true
}
```

Request 1–20 positions. Each returned frame is the first decodable frame at or
after that playback position, within the bounded decode interval. The result
keeps the requested time **and actual decoded PTS**, actual filter time base,
playback time, stream, dimensions and SHA-256 of the PNG. A nearest keyframe is
never relabeled as the requested position. No frame in the interval is an
unavailable result. Frame retrieval does not run OCR or captions.

Frames are downscaled to at most 2 MP and 4 MiB each. The shared request budget
limits aggregate work to 40 MP and encoded output to 32 MiB. A single request
deadline covers admission, metadata, decoding and any optional analysis.

## Sampled observations

Requested OCR and captions sample scene representatives only. OCR records
frame-pixel coordinates, geometry granularity, provider/model provenance and
truncation. It supports text observed at that timestamp, not continuous scene
coverage. Timeline responses contain no image blocks.

`caption: true` uses only the explicitly configured **local-command** vision
adapter. Without one, captions are unavailable. The document VLM is an OCR
recognizer, not a general video captioner. The successful manifest and adapter
cache reuse one description per representative frame; reads, outline navigation
and extra frame retrieval do not recaption it. Partial or failed descriptions
are not cached as successful completion.

Missing optional tools/models produce per-component gaps. Source denial,
malformed media and hash mismatches are operational errors. Decoder/provider
failure or budget exhaustion stops subsequent representative work while keeping
already extracted timeline evidence and truthful partial status.

## Verification

Pure Rust fixtures cover clock offsets, VFR PTS parsing, cue endpoints/overlaps,
window limits and sampled geometry. Real FFmpeg acceptance runs **only on a
disposable CI runner**, without models or customer media:

```sh
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/tmp/anymd-video-target \
  cargo test -p anymd-formats --test video_timeline -- --ignored
```

The real fixture creates tiny lossless media with a hard cut, a nonzero PTS
origin, a delayed audio stream, overlapping subtitles and VFR frame intervals.
It compares returned decoded timestamps and frame hashes. Adapter fixtures prove
plumbing only, not OCR/caption quality. Shared routing, admission/cache/cursor,
provider and clean-client acceptance are checked with the integrated feature.

## Shared request behavior

Sources and subtitle sidecars are admitted before cache lookup. Local inputs are
snapshotted and explicit URLs are fetched once through the existing URL policy;
all evidence hashes the bytes actually processed. A supervised native request
worker bounds decoding, preinstalled ASR, sampled OCR and local captions under
one deadline. No timeline request grants model download permission.

`read` continuation cursors bind to the source and full timeline manifest. Repeat
the same timeline/OCR/transcript options; changed evidence invalidates the cursor.
Outline extracts only navigation metadata, even when `caption` is true. Frames,
manifest JSON and successful caption records live in the existing generated-image
cache and share its age/size pruning. Incomplete components are not cached as
completed timelines. Missing caption adapters remain explicit gaps.

The hosted CI gate explicitly executes real FFmpeg fixtures and native MCP
timeline/frame/read/outline requests. Mock descriptions test adapter/cache plumbing
only, not caption quality.
