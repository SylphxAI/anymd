# Publish status — anymd

| Field | Value |
| --- | --- |
| **Canonical npm** | `@sylphx/anymd` |
| **Canonical bin** | `anymd` |
| **MCP registry name** | `io.github.SylphxAI/anymd` |
| Source tip version | `5.0.0` (this repository) |
| Registry (live) | may lag tip — verify with `npm view @sylphx/anymd version` |
| Aliases (same version) | `@sylphx/citra` (bin `citra`), `@sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`) |
| Auth | npm trusted publishing (GitHub OIDC) from `publish-npm.yml`; no long-lived npm token |

## Install (canonical)

```bash
npm i -g @sylphx/anymd
# or
npx @sylphx/anymd
```

## Former names (aliases)

`@sylphx/citra` and `@sylphx/pdf-reader-mcp` are live aliases of
`@sylphx/anymd`, published at the same version by `publish-npm.yml`. OIDC authenticates only `npm publish`, so that workflow
reports a lingering deprecation notice rather than clearing it; an owner clears
it by hand:

```bash
npm deprecate "@sylphx/pdf-reader-mcp@*" ""
npm deprecate "@sylphx/citra@*" ""
```

Trusted publisher, identical for all eight packages (`@sylphx/anymd`, the five
`@sylphx/anymd-<platform>` natives, `@sylphx/citra`, `@sylphx/pdf-reader-mcp`):
GitHub Actions, organization `SylphxAI`, repository `anymd`, workflow
`publish-npm.yml`, no environment. The `use_token_fallback` input publishes with
the `NPM_TOKEN` secret instead and exists only until every package trusts the
workflow; afterwards the secret is deleted.

Publish authority: Changesets through `release.yml`, then the admission-gated
`publish-npm.yml` artifact path: natives, then `@sylphx/anymd`, then the two
alias packages. There is no republish or unpublish workflow.

A release is closed only after all five native packages and the umbrella package
are read back at one exact version, the installed `anymd` launcher initializes
with that version, the N-1 → N update and uninstall checks pass, and a GitHub
release at the publishing source SHA triggers canonical MCP Registry publication.
The registry workflow then reads back active `io.github.SylphxAI/anymd` metadata
and deprecates every version of the retired MCP Registry identities
(`io.github.SylphxAI/citra`, `io.github.SylphxAI/pdf-reader-mcp`). Cross-build
success is artifact evidence, not host-runtime parity.
