---
"@sylphx/citra": patch
---

Carry the repository identity migration into the published metadata and the queued dependency updates.

The repository moved to `SylphxAI/citra` (retired slug redirects), and with it the GitHub Pages path behind `websiteUrl`/`homepage`. The previous release's published metadata is an immutable snapshot and still names the old path, and neither npm nor the MCP Registry allows rewriting it — npm manifests are immutable and the registry grants `edit` on an existing version only to admins (`github-oidc` mints `publish` only). So this release is what carries the corrected pointers onto `latest`.

Also carries two dependency updates already on `main` and re-pinned in the release review: `rmcp` 3.2.0 → 3.4.0 and `pdf-extract` 0.12.0 → 0.12.1.
