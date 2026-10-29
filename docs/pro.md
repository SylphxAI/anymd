---
title: "anymd Pro: verifiable video evidence and cite-check for AI agents"
titleTemplate: false
description: Let your agent show its evidence. Exact video frames with timestamps and hashes, and offline checks that a quote is on the page it cites. Current one-time pricing from Money, works offline.
image: https://sylphxai.github.io/anymd/og-pro.png
aside: false
sidebar: false
editLink: false
lastUpdated: false
prev: false
next: false
---

# Let your agent show its evidence

**anymd Pro** adds two things to the free anymd MCP server: exact video frames your agent can cite, and **cite-check**, which proves a quote is on the page and at the spot it claims. Everything runs on your machine. <a href="https://sylphxai.github.io/anymd/pricing" target="_self">See current pricing</a>.

<ProBuy />

<p class="pro-trust">Current one-time price and tax are shown on the <a href="https://sylphxai.github.io/anymd/pricing" target="_self">pricing page</a> · licence never expires · works offline, no account · <a href="/anymd/legal/pro-terms">Terms</a></p>

Requires anymd 8.4.0 or later (`npx -y @sylphx/anymd@latest --version`). The anymd core stays free and MIT licensed.

## Why it matters

When an agent summarises a deposition video, an earnings call, a lecture or a contract PDF, you still have to trust it. Pro gives the agent something you can check.

### Verifiable video evidence

- **Timeline.** `inspect` with `video_timeline` returns an ordered, bounded timeline for a video: scenes, chapters, and subtitle or transcript cues with their start and end times, tied to the source file's SHA-256.
- **Frames.** `render_frame` returns the decoded frame at the times you ask for. Each frame records the requested time, the actual decoded timestamp and a SHA-256 of the PNG, so a citation like "at 01:17 the slide shows X" can be re-checked. A nearby keyframe is never passed off as the requested moment.
- **Honest limits.** Scene changes are FFmpeg-detected cuts, a heuristic and not a semantic reading of the video. A timeline covers at most ten minutes per request. No model is downloaded.
- `read` and `outline` also take the `timeline` option, so the same scenes appear as document sections.

[How video timelines work](/guide/video-timeline)

Try it locally: [extract a video frame](/tasks/extract-video-frame) or
[build a video timeline](/tasks/build-video-timeline), with free and Pro examples.

### Cite-check

- `inspect` with `cite_check` takes a PDF and a list of citations (quote, page, bounding box) and returns one verdict each: `verified_exact`, `verified_normalized`, `unmatched` or `insufficient_evidence`.
- It checks that the quote occurs in the text extracted from that page, inside that box. No model judges it, and when the evidence is not enough (a scan with no OCR, missing geometry) it says so rather than guessing.
- A verified result proves the quote is **at that location in the document**. It does not prove the statement is true, and it does not vouch for OCR accuracy.

[How cite-check works](/guide/cite-check)

Try it locally: [check a PDF citation](/tasks/check-pdf-citation), starting with
a generated one-page PDF and a free text search.

### Who it is for

Agents and people who have to point at a source and be believed: legal review, research, finance and media work.

## Price

**Current one-time price, including any applicable tax.** No subscription. One licence per person, no expiry, future releases included. Instant delivery after payment: copy your licence token from the checkout confirmation page. We also email a link to retrieve it.

<ProBuy label="Buy anymd Pro" />

Buy from the terminal: `anymd pro buy` opens the checkout in your browser and turns the payment into a licence token on this machine, so there is nothing to copy. It prints the Pro page and the purchase command instead if the checkout cannot be reached.

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
Offline, and no account. Your licence is checked on your machine against a public key built into anymd. Nothing is sent anywhere when you use Pro.

**Is the core still free?**
Yes. anymd stays MIT licensed. Reading, outlining, searching and inspecting documents, OCR, transcripts, the MCP server and the CLI stay free and ungated, and nothing that was free has moved to Pro. Ordinary `read` and `outline` of video (metadata, chapters, subtitles) are still free.

**What happens without a licence?**
A Pro operation returns a short message with the current one-time price, `anymd pro buy` and a link to this page, and does no work. Nothing else changes.

**Which version do I need?**
anymd 8.4.0 or later. `anymd pro status` tells you whether Pro is active.

**Can my team use one licence?**
A licence is for one person. Buy one for each person who uses Pro.

**Refunds?**
Pro is digital content delivered immediately: at checkout you ask us to supply it straight away and acknowledge that you lose your 14-day right to cancel once your licence token is delivered. If Pro doesn't work as described and we can't fix it, email hi@sylphx.com and we'll put it right or refund you. See the [terms](/legal/pro-terms).

**Who sells it?**
Sylphx Limited, a company registered in England and Wales (company number 16438428). See the [anymd Pro terms](/legal/pro-terms) and [Privacy](/legal/privacy).

<ProTracking page="pro" />
