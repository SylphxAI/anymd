# Citra — brand-sole publish (hard cut)

**Publish authority:** this repository only.

| Field | Value |
| --- | --- |
| Brand | **Citra** |
| **Canonical npm** | `@sylphx/citra` |
| **Canonical bin** | `citra` |
| **MCP registry name** | `io.github.SylphxAI/citra` |
| Retired package ID | `@sylphx/pdf-reader-mcp` (historical pins only) |
| GitHub repository | `SylphxAI/citra` (renamed 2026-09-22 from `SylphxAI/pdf-reader-mcp`, which redirects) — source location, **not** a product identity |

## Policy (clean break)

1. **One product / one identity:** `@sylphx/citra` is the only supported install path.
2. Retired `@sylphx/pdf-reader-mcp` must not be a current install CTA or publish target.
3. Do not create or publish an alias package. Git history and immutable historical
   registry versions preserve migration evidence without a second product path.
4. Native optional dependencies use the Citra package family and their versions
   **must** match the Citra umbrella version.
5. The GitHub release, npm provenance, installed-launcher proof, and MCP Registry
   record must bind the same version and source SHA before release closeout.
6. The repository slug is not part of this contract — `io.github.SylphxAI/citra`
   was published while the repository was still named `pdf-reader-mcp`, so the
   registry name never bound the slug. It was renamed to `SylphxAI/citra` on
   2026-09-22 to retire the last old identifier anyway. GitHub redirects every
   old location **except project site URLs**, so the rename moved the Pages path
   behind `websiteUrl`/`homepage` and required updating `base` in the VitePress
   config alongside it. The retired slug `SylphxAI/pdf-reader-mcp` stays empty
   so the git, issue, and PR redirects keep working.

## User install

```bash
npm i -g @sylphx/citra
# or
npx @sylphx/citra
```

## Deprecate transitional (registry auth required)

```bash
npm deprecate @sylphx/pdf-reader-mcp@"*" \
  "Retired install CTA. Use @sylphx/citra (bin: citra)."
```
