# anymd — local agent notes only

anymd (formerly Citra / pdf-reader-mcp): canonical npm `@sylphx/anymd`,
repository `SylphxAI/anymd` (old slugs `SylphxAI/citra` and
`SylphxAI/pdf-reader-mcp` redirect). `@sylphx/citra` and
`@sylphx/pdf-reader-mcp` are thin aliases published at the same version.

Doctrine, Mission Control, and GroundAtlas package dogfood are retired
historical lineage and must not be loaded as current instruction or live-state
authority.

Local truth:

- `PROJECT.md` — project facts and human projection

## Boundary hazards

- Local-first privacy: do not upload documents or call remote providers unless
  the caller explicitly selects a remote provider adapter.
- No hosted auth, billing, storage, tenancy, durable work, or customer-account
  state in this package.
- No direct provider secrets, Gateway credentials, or product-specific model
  routing.
- Optional OCR/vision/region providers stay behind typed adapters with
  fail-closed defaults.
- Public MCP schemas are contracts — version and regression-test option/output
  shapes. Production schema authority is the Rust MCP server, not residual TS.
- Preserve page/region/source provenance on extraction and analysis outputs.
- Package publishing is Changesets / bot-owned; do not publish from a human shell.
- Never commit secrets, private documents, or customer data.

## Local commands

```bash
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run check
bun run build
bun run docs:build
bun test test/project-control.test.ts
bun run check:ts-production-absence
```

## Validation notes

- Prefer the **narrowest** affected check before full workspace runs.
- Report layers honestly: local diff · trunk land · package publish · registry
  readback (do not collapse).
- Do not claim release or adoption completion while residual TS, version skew,
  trunk CI, registry, or clean-install gaps remain open.

## Backend false-authority fence

If this repository has completed a **Rust backend** cutover:

1. Production backend behavior authority is the Rust crate/binary path declared
   in package `bin` / native optional packages / Docker ENTRYPOINT when present.
2. Residual TypeScript service trees or alternate TS engines are **not** product
   authority unless explicitly proven still on the live path.
3. Do not "fix production" by editing residual TypeScript and assuming
   deploy/runtime will pick it up.
4. Prefer deleting residual TS backend trees after sole-Rust proof; keep history
   in Git.
5. Intentional TypeScript packaging wrappers and native-binding surfaces may remain.

### Repo-specific note

Native Rust engine is product authority; npm `dist/runtime-entry.js` and
`dist/pure-rust.js` are packaging/launcher surfaces only, not an alternate PDF
backend. Historical TypeScript LKG (if needed) is the external pin
`@sylphx/pdf-reader-mcp@3.0.14`, not residual source under `src/`.

## Residual TypeScript (non-production)

`src/pdf/**`, `src/handlers/**`, `src/legacy-engine-runtime.ts`, and related trees are **oracle/benchmark-only**.
They are **not** shipped (`package.json` files allowlist) and are **not** production authority.
Do not restore them as a production runtime. Prefer deleting after oracle migration to Rust-only.
