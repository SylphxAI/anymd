---
title: "anymd Pro: video moments you can cite, quotes you can check"
titleTemplate: false
description: Cite video moments and check PDF quotes on your own machine. Buy a 100-year Pro licence with one payment. See the current price before you buy.
image: https://sylphxai.github.io/anymd/og-pro.png
aside: false
sidebar: false
editLink: false
lastUpdated: false
prev: false
next: false
---

# Video moments you can cite, quotes you can check

Give your agent the video frame behind a claim, and check whether a PDF quote appears where its citation says it does. Both work on your machine. <a href="https://sylphxai.github.io/anymd/pricing" target="_self">See current pricing</a>.

<ProBuy />

<p class="pro-trust">Pay once. Use Pro offline for 100 years, with no account required. The <a href="https://sylphxai.github.io/anymd/pricing" target="_self">checkout price</a> includes applicable tax. <a href="/anymd/legal/pro-terms">Terms apply</a>.</p>

Works with anymd 8.4.0 or later. The core stays free and MIT licensed. Check your version in the [getting started guide](/guide/getting-started).

## Why it matters

Reviewing a video summary or a quoted passage should not mean searching the whole source again. Pro takes you back to the exact frame or page, so you can check an answer before you use it.

### Verifiable video evidence

- **Find your way through a video** with scene changes, chapters and subtitle or transcript timings. Each timeline is linked to its source file.
- **See the frame behind a video citation.** Each image includes the requested time and the actual decoded timestamp, so you can check which moment it shows.
- **Know the limits.** Timelines cover up to 10 minutes per request. Scene markers detect visual cuts; they do not explain what the video means. No AI model download is needed.

[How video timelines work](/guide/video-timeline)

Try it locally: [extract a video frame](/tasks/extract-video-frame) or
[build a video timeline](/tasks/build-video-timeline), with free and Pro examples.

### Cite-check

- **Check each PDF quote against its cited page and area.** See whether it matches, doesn't match, or needs more evidence.
- Checks use the text and its position on the page, not an AI opinion. A scan without OCR or missing text positions may leave a citation unconfirmed.
- A match confirms where the quote appears, not whether it is true. OCR errors can still affect the result.

[How cite-check works](/guide/cite-check)

Try it locally: [check a PDF citation](/tasks/check-pdf-citation), starting with
a generated one-page PDF and a free text search.

### Who it is for

Developers building agents that cite their sources. Writers checking quotes. Researchers, legal reviewers and finance teams who need to show where an answer came from.

## Price

**Current one-time price, including any applicable tax.** Choose from 16 currencies at checkout. No subscription. One licence per person, valid for 100 years, with future releases included. Instant delivery after payment: copy your licence token from the checkout confirmation page. We also email a link to retrieve it.

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

Using a server or CI? Set `ANYMD_PRO_TOKEN`. It takes priority over a saved licence; `anymd pro status` confirms activation without displaying your key.

## FAQ

**Does it work offline? Do I need an account?**
Use Pro offline, without an account. Licence checks stay on your machine.

**Is the core still free?**
Yes. anymd stays MIT licensed. Reading, outlining, searching and inspecting documents, OCR, transcripts, the MCP server and the CLI stay free and ungated, and nothing that was free has moved to Pro. Ordinary `read` and `outline` of video (metadata, chapters, subtitles) are still free.

**What happens without a licence?**
Not on Pro yet? Free features keep working. If you try a Pro feature, you'll see the price and how to buy before that feature runs.

**Which version do I need?**
anymd 8.4.0 or later. `anymd pro status` tells you whether Pro is active.

**Can my team use one licence?**
A licence is for one person. Buy one for each person who uses Pro.

**Refunds?**
Pro is digital content delivered immediately: at checkout you ask us to supply it straight away and acknowledge that you lose your 14-day right to cancel once your licence token is delivered. If Pro doesn't work as described and we can't fix it, email hi@sylphx.com and we'll put it right or refund you. See the [terms](/legal/pro-terms).

**Who sells it?**
Sylphx Limited, a company registered in England and Wales (company number 16438428). See the [anymd Pro terms](/legal/pro-terms) and [Privacy](/legal/privacy).
