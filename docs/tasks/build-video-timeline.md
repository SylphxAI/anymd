---
title: Build a video timeline with scenes and timestamps
description: Turn a local video into bounded scene and cue evidence over MCP. Compare free metadata reads with anymd Pro timelines using released anymd 8.5.5.
---

# Build a video timeline with scenes and timestamps

Use a timeline when you need an ordered record of a video interval before
choosing moments to inspect. anymd Pro combines detected cuts, chapters and
available subtitle or transcript cues with the source hash and timing evidence.
The cuts are an FFmpeg heuristic, not a semantic account of what happens.

The free MIT core still reads video metadata, chapters and subtitles without
a licence. [See current Pro pricing](https://sylphxai.github.io/anymd/pricing?utm_source=docs_build_video_timeline).

## Create a two-scene fixture

Use Node.js 18+, npm, a POSIX shell and FFmpeg (including `ffprobe`) on your PATH.
No model or remote provider is used in this example.

```sh
mkdir anymd-timeline && cd anymd-timeline
npm install --no-save @sylphx/anymd@8.5.5
curl -fsSLO https://sylphxai.github.io/anymd/examples/mcp-call.mjs
ffmpeg -hide_banner -loglevel error \
  -f lavfi -i 'color=c=red:size=320x240:rate=10:duration=1' \
  -f lavfi -i 'color=c=blue:size=320x240:rate=10:duration=1' \
  -filter_complex '[0:v][1:v]concat=n=2:v=1:a=0[v]' -map '[v]' -c:v ffv1 demo.mkv
```

The [MCP client](/examples/mcp-call.mjs) runs the installed release with file
access restricted to the current directory. It prints each tool result. You can
also send the JSON arguments through another MCP client.

## Free: read metadata without scene detection

```sh
node mcp-call.mjs read '{"source":"demo.mkv","ocr":false,"images":"none"}'
```

Expect video metadata. The generated fixture has no chapters, subtitles or audio,
so there is no transcript to read. A free container read is not a scene timeline.
For a video with embedded subtitles, ordinary `read` remains useful without Pro.

## Pro: inspect a bounded interval

With an [existing licence activated](/pro#activate), run:

```sh
node mcp-call.mjs inspect '{"operation":"video_timeline","sources":[{"path":"demo.mkv"}],"timeline":{"start_ms":0,"end_ms":2000,"max_scenes":20,"caption":false}}'
```

Expect a source hash, requested and processed ranges, timing mapping, the
initial scene and a detected cut at the red-to-blue transition around 1000 ms.
Check the detection status and component gaps, not just the scene count.
There are no cue tracks in this fixture; their absence is not lost dialogue.
No captions are generated and no ASR model is downloaded.

Without a licence, this request returns the Pro message and does no timeline
work. Missing FFmpeg or unavailable components must not be mistaken for a
complete timeline.

## Read the same timeline as sections

The Pro timeline option also works with `outline` and `read`:

```sh
node mcp-call.mjs outline '{"source":"demo.mkv","timeline":{"start_ms":0,"end_ms":2000,"max_scenes":20,"caption":false}}'
node mcp-call.mjs read '{"source":"demo.mkv","timeline":{"start_ms":0,"end_ms":2000,"max_scenes":20,"caption":false},"ocr":false,"images":"none"}'
```

`outline` returns navigation headings; `read` projects timeline evidence into
document sections. Repeat the same options when continuing a read cursor.
To inspect a particular visual moment, [request a decoded frame](./extract-video-frame).

## Limits to preserve in your application

- `end_ms` is required. Each interval is positive, half-open and at most ten minutes.
- `max_scenes` cannot exceed 20. Excess cuts are coalesced into the final scene
  with partial detection status, not silently presented as complete detection.
- Scene-score 0.4 cuts are heuristic, not semantic scenes or confidence scores.
- Subtitle and ASR cues have their own timing granularity. A sampled OCR result
  or caption is evidence from one representative frame, not continuous coverage.
- Captions need an explicitly configured local-command adapter. Missing adapters
  remain unavailable; anymd has no built-in video captioner.

The [timeline reference](/guide/video-timeline) covers clocks, sidecars, budgets
and partial results in detail.

[See current Pro pricing](https://sylphxai.github.io/anymd/pricing?utm_source=docs_build_video_timeline).
