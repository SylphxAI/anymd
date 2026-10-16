---
description: "anymd is the free, local mode; Sylphx AI parse is the hosted, metered mode of the same crates. What each costs and which to pick."
---

# Local and hosted

anymd has two modes, and they are the same engine. This page says which is which, what each costs, and why the crate APIs stay stable.

## Local: anymd (free, MIT, this repository)

The `anymd` binary is the MCP server and the CLI. It converts PDF, Word, PowerPoint, Excel, EPUB, HTML and web pages, images (OCR), audio and video on your machine. Nothing is uploaded, no account or API key is needed, and the license is MIT.

- One Rust binary (`crates/anymd`), plus the converter crates below it.
- Every feature described on this site, including local doc-VLM OCR and transcripts.
- No network call unless you pass a URL, and then only that URL is fetched.

This is the default mode, and it is the one this documentation describes. It is not a trial and not feature-limited: nothing that was free has moved to the hosted mode.

## Hosted: Sylphx AI parse (metered)

[Sylphx AI parse](https://sylphx.com/ai) is the hosted document-parse service, for callers that do not want a local runtime: a server, an agent platform or a product that sends a document and gets page-indexed Markdown back. It is billed per page and needs a Sylphx AI account. The billing, accounts and service live in the Sylphx platform; the parser lives here.

The hosted service imports the `anymd-core`, `anymd-pdf` and `anymd-formats` crates at a pinned published version. It is not a fork and not a second parser: a fix merged here reaches both the local binary and the hosted service.

## Which to pick

| You want | Use |
|---|---|
| Privacy, offline use, no account, no per-page cost | Local `anymd` (this repository) |
| No local runtime, or conversion inside a server or platform | Hosted [Sylphx AI parse](https://sylphx.com/ai) |
| Both, with the same output | Local for the developer, hosted for the service; the crates are the same |

Local and hosted produce the same Markdown from the same document, because the conversion code is the same code. Hosted adds the service around it: an endpoint, metering and a model runtime for text-less pages.

## API stability for importers

The public converter API of `anymd-core`, `anymd-pdf` and `anymd-formats` is stable within a major version. The hosted service pins an exact published version, so a patch or minor release here never forces a change there:

- a patch release fixes behaviour and never changes the public API;
- a minor release adds API and never breaks an existing one;
- a breaking API change ships only in a major version, and the service moves to it deliberately, not automatically.

This is why the two modes cannot drift into separate engines: there is one converter, one set of crates, and one version the service pins.

## anymd Pro

[anymd Pro](/pro) is a separate, one-off licence for two local-only extras: video timelines with exact hashed frames, and cite-check. It runs offline with no account, works with future releases, and is not affected by the hosted service. The free core and the Pro operations both stay in this repository's local binary.
