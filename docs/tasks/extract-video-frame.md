---
title: Extract a video frame at a timestamp with anymd
description: Retrieve a decoded video frame over MCP, keeping the requested time, actual PTS and PNG hash. Runnable free and Pro examples with anymd 8.5.5.
---

# Extract a video frame at a timestamp

If you need to cite what a video shows at 1.031 seconds, a nearby keyframe is
not the same evidence. anymd Pro's `render_frame` operation returns the first
decodable frame at or after the requested playback time within its bounded
decode interval. It keeps both the request and the actual decoded timestamp.
It does not promise a frame exists at every millisecond.

The free MIT core reads video metadata, chapters and subtitles. Pro adds
timestamped decoded frames and [bounded timelines](./build-video-timeline).
[See current Pro pricing](https://sylphxai.github.io/anymd/pricing?utm_source=docs_extract_video_frame).

## Make a small local video

Use Node.js 18+, npm and FFmpeg (including `ffprobe`) on your PATH. These
commands use a POSIX shell. Everything after installation runs locally; no
model, API key or transcription download is needed.

```sh
mkdir anymd-frames && cd anymd-frames
npm install --no-save @sylphx/anymd@8.5.5
curl -fsSLO https://sylphxai.github.io/anymd/examples/mcp-call.mjs
ffmpeg -hide_banner -loglevel error -f lavfi \
  -i 'testsrc2=size=320x240:rate=10:duration=2' -c:v ffv1 demo.mkv
```

The [small MCP client](/examples/mcp-call.mjs) starts the installed release
with access restricted to this directory. It prints the tool result and saves
PNG image blocks as `frame-1.png`, `frame-2.png`, and so on in this directory.
Those names are overwritten on the next image request; rename files you want
to keep. The JSON arguments below also work in any MCP client connected to anymd.

## Free: read what is already in the container

```sh
node mcp-call.mjs read '{"source":"demo.mkv","ocr":false,"images":"none"}'
```

Expect video metadata for this two-second fixture. It has no subtitle or
audio tracks. This does not extract a decoded frame or verify a visual claim.
On your own video, ordinary `read` can also expose embedded subtitles and chapters.

## Pro: request the frame, not a keyframe thumbnail

Activate an existing licence using the [activation instructions](/pro#activate)
and restart your MCP client. The example script starts a fresh server each time.
Do not put a licence token in a source file or a shared command transcript.

```sh
node mcp-call.mjs inspect '{"operation":"render_frame","sources":[{"path":"demo.mkv"}],"timestamps_ms":[1031,1500],"include_image":true}'
```

With an active licence, inspect the returned frame records for requested time,
decoded PTS, time base, playback time, dimensions and PNG SHA-256. The 10 fps
fixture has frames every 100 ms: the first request should resolve to 1100 ms,
not be relabelled 1031 ms. The second should resolve to 1500 ms. Compare the
saved PNGs with the hashes in the result; do not substitute a hash from a
different FFmpeg build or source file.

Without a licence, the same valid request returns the Pro message and does no
frame work. That is not a successful extraction, even though the MCP response
is a normal text result rather than a tool error.

## What a frame can support

A decoded frame supports an observation at its actual timestamp. It does not
establish what was visible between frames, OCR accuracy or the truth of a
caption. Frame retrieval itself does not run OCR or captions. An unavailable
frame stays unavailable.

Request 1–20 timestamps. Output is downscaled to at most 2 MP and 4 MiB per
frame, with a shared 40 MP / 32 MiB request budget. Keep those limits in mind
before treating the image as an original-resolution source.

See the [video reference](/guide/video-timeline#frames) for decoding and budget
semantics, or [build a timeline](./build-video-timeline) before picking frames.

[See current Pro pricing](https://sylphxai.github.io/anymd/pricing?utm_source=docs_extract_video_frame).
