---
title: anymd Pro
description: Verifiable video evidence and cite-check for agents doing legal, research, finance or media work. US$29 once, verified offline.
aside: false
---

# anymd Pro

**Let your agent show its evidence.** Pro adds two capabilities to anymd: a video timeline with exact frames, and cite-check, which proves a quote is on a given page at a given location.

<ProBuy />

US$29, once. Your licence token arrives by email.

## Why it matters

When an agent summarises a deposition video, an earnings call recording, a lecture or a contract PDF, you still have to trust it. Pro gives the agent something you can check.

### Verifiable video evidence

- **Timeline.** `inspect` with `video_timeline` returns an ordered, bounded timeline for a video: scenes, chapters, and subtitle or transcript cues with their start and end times, tied to the source file's SHA-256.
- **Frames.** `render_frame` returns the decoded frame at the times you ask for. Each frame records the requested time, the actual decoded timestamp and a SHA-256 of the PNG, so a citation like "at 01:17 the slide shows X" can be re-checked. A nearby keyframe is never passed off as the requested moment.
- **Honest limits.** Scene changes are FFmpeg-detected cuts, a heuristic and not a semantic reading of the video. A timeline covers at most ten minutes per request. No model is downloaded.
- `read` and `outline` also take the `timeline` option, so the same scenes appear as document sections.

[How video timelines work](/guide/video-timeline)

### Cite-check

- `inspect` with `cite_check` takes a PDF and a list of citations (quote, page, bounding box) and returns one verdict each: `verified_exact`, `verified_normalized`, `unmatched` or `insufficient_evidence`.
- It checks that the quote occurs in the text extracted from that page, inside that box. No model judges it, and when the evidence is not enough (a scan with no OCR, missing geometry) it says so rather than guessing.
- A verified result proves the quote is **at that location in the document**. It does not prove the statement is true, and it does not vouch for OCR accuracy.

[How cite-check works](/guide/cite-check)

### Who it is for

Agents and people who have to point at a source and be believed: legal review, research, finance and media work.

## Price

**US$29, once.** No subscription.

<ProBuy label="Buy anymd Pro" />

## Activate

Run it once:

```bash
anymd pro activate '<your token>'
anymd pro status
```

or set the token in the environment:

```bash
export ANYMD_PRO_TOKEN='<your token>'
```

The environment variable wins over the saved file. `status` shows whether Pro is active; it never prints the token.

## FAQ

**Does it work offline? Do I need an account?**
Offline, and no account. Licences are verified on your machine against a public key built into anymd. Nothing is sent anywhere when you use Pro.

**Is the core still free?**
Yes. anymd stays MIT licensed. Reading, outlining, searching and inspecting documents, OCR, transcripts, the MCP server and the CLI stay free and ungated, and nothing that was free has moved to Pro. Ordinary `read` and `outline` of video (metadata, chapters, subtitles) are still free.

**What happens without a licence?**
A Pro operation returns a short message with a link to this page and does no work. Nothing else changes.

**Refunds?**
Email us by replying to the email your token came in, and we will refund you.

**What do I get for the money?**
The two capabilities above, and you fund development of anymd.

<ProTracking page="pro" />
