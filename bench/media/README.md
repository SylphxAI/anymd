# Embedded-video subtitle benchmark

This benchmark compares the published **anymd 8.5.1** native CLI with **FFmpeg
6.1.1** on two authored fixture datasets. It measures embedded subtitle text and
start timestamps, not scene descriptions, frame OCR, audio transcription, subtitle
end times, or metadata accuracy. The image comparison is published separately in
[the benchmark guide](../../docs/guide/benchmarks.md#image-and-video-extraction).

## Why FFmpeg

For extracting an existing subtitle stream, a direct FFmpeg demux/transcode is the
lossless reference alternative: it preserves all 12 authored cues in each dataset.
It is also the subtitle extractor anymd calls internally. This comparison measures
what anymd's Markdown rendering retains on top of that extractor, not independent
recognition engines. FFmpeg is not an alternative for image-to-Markdown OCR.

## Datasets and method

[`corpus.json`](corpus.json) contains four cases, each with three cues: English,
multiline/punctuation/numbers, mixed Chinese/Japanese/English, and a repeated
caption at a different timestamp. Each case is muxed into both:

- `mp4-mov-text`: MPEG-4 video with an embedded `mov_text` subtitle stream.
- `matroska-subrip`: Matroska video with an embedded SubRip stream.

All eight clips are three-second generated black videos (160×90, 10 fps). The
fixture text is authored here under this repository's MIT license. These are small
regression datasets, not representative samples of films or lectures. No media is
downloaded. Source SRT files have different stems from the videos, so anymd cannot
read them as subtitle sidecars.

A match requires exact cue text after whitespace normalization **and** its start
second. Matching uses a multiset, so a duplicate cannot earn extra credit; a
repeated caption at a different start second must survive separately. Precision
is matches / extracted cues; recall is matches / expected cues. No case folding,
translation or punctuation removal is applied. Starts are whole seconds in these
fixtures; subsecond accuracy and end timestamps are not measured.

The FFmpeg command is `ffmpeg -nostdin -v error -i CLIP -map 0:s:0 -c:s srt -f srt -`.
The anymd command is `anymd CLIP` with default options. Each process runs once,
sequentially, on the same Linux x86_64 standard Build lease. Runtime includes
process startup; muxing, installation and downloads are excluded. anymd also
probes metadata and renders Markdown, while FFmpeg only extracts subtitles, so
the time comparison is not equal work. CPU model, repeated-run variance and peak
memory were not measured. Do not treat these short runs as throughput guarantees.

## Results

Measured at **2026-10-08T02:07:30Z**. [`results.json`](results.json) records the
versions, corpus SHA-256, per-clip video digests, exit codes, complete outputs,
counts and elapsed seconds. All 16 extraction processes exited 0.

| Dataset (4 clips, 12 cues each) | anymd matches | anymd precision / recall | FFmpeg matches | FFmpeg precision / recall | anymd total time | FFmpeg total time |
|---|---|---|---|---|---|---|
| MP4 mov_text | 11/12 | 100% / 91.7% | 12/12 | 100% / 100% | 2.108 s | 1.128 s |
| Matroska SubRip | 11/12 | 100% / 91.7% | 12/12 | 100% / 100% | 2.412 s | 1.155 s |

anymd returns metadata plus timestamped Markdown, but drops the second identical
caption in the repeated-caption case on both containers. Its rolling-caption
cleanup removes lines present in the preceding cue. FFmpeg preserves both cues.
The other three cases retain all text/start pairs, including CJK and multiline
text. This is a rendering trade-off exposed by the benchmark, not an ASR error.

## Reproduce

Install Python 3 and FFmpeg. Obtain the published native binary, pinned to 8.5.1,
from `@sylphx/anymd-linux-x64-gnu@8.5.1` (or the matching platform package). Pass
its absolute path; a JavaScript launcher also needs Node and adds launcher cost.

From the repository root, on a build runner:

```bash
python3 -I -m unittest discover -s bench/media -p 'test_*.py' -v
python3 -I bench/media/run.py --anymd /absolute/path/to/anymd \
  --work /new/empty/media-fixtures --out bench/media/results.json
```

`--work` must not exist before the run. The harness generates fixtures there,
keeps their source SRT and extracted outputs, and writes raw results to `--out`.
Versions are recorded rather than silently substituted; use FFmpeg
`6.1.1-3ubuntu5` to match this published run. Regenerated container bytes may differ
with another FFmpeg build; the measured run's digests identify its exact inputs.
