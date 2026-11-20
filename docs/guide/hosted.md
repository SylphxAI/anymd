---
description: "anymd is the free, local MIT core; anymd Pro ($29 once) adds offline PDF cite_check and verifiable video timelines and frames."
---

# Local core and anymd Pro

anymd runs locally on your machine. This page explains what is in the free MIT core, what [anymd Pro](/pricing) ($29 once) adds, and how the underlying Rust crates stay semver-stable for downstream importers.

## Local core: anymd (free, MIT, this repository)

The `anymd` binary is the MCP server and the CLI. It converts PDF, Word, PowerPoint, Excel, EPUB, HTML and web pages, images (OCR), audio and video on your machine. Nothing is uploaded, no account or API key is needed, and the license is MIT.

- One Rust binary (`crates/anymd`), plus the converter crates below it.
- Every conversion feature described on this site, including local doc-VLM OCR and transcripts.
- No network call unless you pass a URL, and then only that URL is fetched.

This is the default mode, and it is not a trial and not feature-limited: nothing that was free has moved to Pro.

## Verifiable evidence: anymd Pro ($29 once)

[anymd Pro](https://anymd.sylphx.com/pricing) is a one-time, 100-year offline licence (US$29 or the local price in 16 currencies, tax included) for agents and teams that need verifiable source evidence:

- **PDF `cite_check`**: verify that a quoted passage appears on the cited page and area using extracted text and coordinates, not an AI opinion.
- **Video timelines and frames (`video_timeline`, `render_frame`)**: bounded scene/chapter/transcript timelines and exact decoded frames with SHA-256 hashes.

Buy on the web at [anymd Pro pricing](https://anymd.sylphx.com/pricing) or run `anymd pro buy` in your terminal.

## Which to pick

| You want | Use |
|---|---|
| Fast local conversion, search, outline, OCR and transcripts with no account | Free local `anymd` (MIT) |
| Deterministic PDF `cite_check` and SHA-256 verifiable video frames offline | [anymd Pro ($29 once)](https://anymd.sylphx.com/pricing) |
| Embedding the converter in a Rust service or platform | `anymd-core`, `anymd-pdf` and `anymd-formats` crates |

## API stability for importers

The public converter API of `anymd-core`, `anymd-pdf` and `anymd-formats` is stable within a major version. Downstream services pin an exact published version, so a patch or minor release here never forces a change there:

- a patch release fixes behaviour and never changes the public API;
- a minor release adds API and never breaks an existing one;
- a breaking API change ships only in a major version, and importers move to it deliberately, not automatically.

This is why there is one converter and one set of crates across local CLI, MCP and embedded deployments.

## Get anymd Pro

[anymd Pro](/pricing) runs 100% offline with no account, works with future releases, and is delivered instantly on the checkout confirmation page or automatically via `anymd pro buy`.