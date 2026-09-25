# anymd — canonical publish

**Publish authority:** this repository only.

| Field | Value |
| --- | --- |
| Brand | **anymd** — any file → clean Markdown for AI agents |
| **Canonical npm** | `@sylphx/anymd` |
| **Canonical bin** | `anymd` |
| **MCP registry name** | `io.github.SylphxAI/anymd` |
| Alias packages (same version) | `@sylphx/citra` (bin `citra`), `@sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`) |
| Retired MCP registry names | `io.github.SylphxAI/citra`, `io.github.SylphxAI/pdf-reader-mcp` (deprecated) |
| GitHub repository | `SylphxAI/anymd` (formerly `SylphxAI/citra` and `SylphxAI/pdf-reader-mcp`, which redirect) — source location, **not** a product identity |

## Policy

1. **One product / one identity:** `@sylphx/anymd` is the canonical install path.
2. The former names `@sylphx/citra` and `@sylphx/pdf-reader-mcp` are thin alias
   packages in `packages/alias-*`. Each depends on `@sylphx/anymd` at the exact
   same version and only runs its launcher. `scripts/release-version.sh` keeps
   them in lockstep; `publish-npm.yml` publishes natives → main → aliases.
3. Native optional dependencies use the `@sylphx/anymd-<platform>` family and
   their versions **must** match the anymd umbrella version.
4. The GitHub release, npm provenance, installed-launcher proof, and MCP Registry
   record must bind the same version and source SHA before release closeout.
5. The repository slug is not part of this contract. GitHub redirects every old
   location **except project site URLs**, so a rename moves the Pages path behind
   `websiteUrl`/`homepage` and requires updating `base` in the VitePress config
   alongside it. Retired slugs stay empty so git, issue, and PR redirects keep
   working.

## User install

```bash
npm i -g @sylphx/anymd
# or
npx @sylphx/anymd
```

## Former names (aliases)

`@sylphx/citra` and `@sylphx/pdf-reader-mcp` are live aliases of
`@sylphx/anymd`, published at the same version by `publish-npm.yml` through npm
trusted publishing. Deprecation is a one-off owner action outside the release;
to clear a notice:

```bash
npm deprecate "@sylphx/pdf-reader-mcp@*" ""
npm deprecate "@sylphx/citra@*" ""
```
