---
title: "PDF Cite-Check & Verifiable Video Frames for AI Agents | anymd Pro"
titleTemplate: false
description: "Prove an AI agent's PDF quote sits on the exact page and bounding box—and extract SHA-256 hashed video frames at exact timestamps. Offline Ed25519 licence, $29 once."
image: https://sylphxai.github.io/anymd/og-pro.png
---

<p class="pro-trust">anymd Pro · $29 one-time offline licence</p>

# Make your AI agent prove the page, box, and video frame it cites.

When an agent summarizes a contract PDF, financial filing, lecture, or deposition video, plain text isn't audit-grade proof. **anymd Pro** adds deterministic `cite_check` (proving a quote exists at an exact physical PDF page and bounding box) and SHA-256-verified video frames—running 100% offline with a single **$29** licence.

<p>
  <a class="pro-buy" href="https://sylphxai.github.io/anymd/pricing?entry_surface=seo_verifiable-video-and-cite-check-ai-agents">Buy anymd Pro ($29 once — offline licence)</a>
  &nbsp;&nbsp;
  <a href="/anymd/guide/cite-check?entry_surface=seo_verifiable-video-and-cite-check-ai-agents">Read the cite-check spec</a>
</p>

## Audit-grade evidence, 100% offline

### 1. Deterministic `cite_check` against extracted page geometry (no LLM judge)

Pass up to **100 citations** (quote, 1-based physical page, and bottom-left PDF `{ left, bottom, right, top }` bounding box) to `inspect` (`operation: "cite_check"`). `anymd` checks native PDF text first (or selected local OCR) and returns an honest verdict per citation: `verified_exact`, `verified_normalized`, `unmatched`, or `insufficient_evidence`.

### 2. Exact-timestamp video frames and bounded timelines with SHA-256 hashes

`inspect` with `video_timeline` builds an ordered timeline of scenes, chapters, and subtitle/transcript cues (up to 10 minutes per request) bound to the video's SHA-256. `render_frame` returns the decoded PNG frame at the requested timestamp with its actual decoded timestamp and PNG SHA-256—never silently substituting a nearby keyframe.

### 3. One-time purchase, offline Ed25519 verification, zero telemetry

Buy once in your local currency across 16 supported currencies (**US$29** baseline, tax inclusive) on the web or straight from your terminal with `anymd pro buy`. Activate with `anymd pro activate '<token>'` or `ANYMD_PRO_TOKEN`; the binary checks your token offline against an embedded Ed25519 public key for 100 calendar years.

## How anymd Pro compares

| Capability | LLM "Check Its Own Quote" | Cloud Video/PDF Platforms | anymd Pro (`anymd >= 8.4.0`) |
|---|---|---|---|
| **Quote verification method** | Probabilistic LLM guess | Proprietary cloud APIs | **Deterministic text + bounding-box check** (`verified_exact` / `insufficient_evidence`) |
| **Video frame proof** | Hallucinated timestamps or nearest keyframe | Server-side upload required | **Exact decoded timestamp + PNG SHA-256** tied to source SHA-256 |
| **Offline &amp; air-gapped use** | Impossible | Account + API key required | **100% offline Ed25519 token check**, zero phone-home |
| **Pricing** | Recurring token cost | Per-minute / monthly seats | **$29 USD once** (16 local currencies at checkout, 100-year single-person licence) |

## FAQ

**What does `cite_check` prove—and what does it NOT claim?**
`cite_check` proves whether a quoted string occurs in extracted text inside the specified physical PDF page and bounding box. No LLM judges it, and if a scanned page lacks OCR or geometry it returns `insufficient_evidence` rather than guessing. It verifies source-location match, not semantic truth.

**Does `anymd Pro` need an account or internet connection after purchase?**
No. After checkout you receive an Ed25519-signed token (`anymd pro activate '<token>'` or `ANYMD_PRO_TOKEN`) verified locally against the public key built into `anymd`. Nothing phones home when you run Pro operations.

**What does `anymd Pro` cost, and is there a subscription?**
It is a single one-time payment—**US$29** or your local currency across 16 supported currencies at checkout (applicable tax included)—covering one person for 100 calendar years including future releases. No subscription.

**Can I buy and activate Pro directly from the terminal?**
Yes. On `anymd 8.4.0+`, run `anymd pro buy` to open checkout in your browser and save the issued licence token automatically on your machine, or [buy at /pricing](https://sylphxai.github.io/anymd/pricing?entry_surface=seo_verifiable-video-and-cite-check-ai-agents).
