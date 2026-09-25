---
'@sylphx/anymd': major
---

Rename to **anymd** — any file → clean Markdown for AI agents.

- The npm package is now `@sylphx/anymd` with the `anymd` bin; the MCP registry
  name is `io.github.SylphxAI/anymd`; the repository is `SylphxAI/anymd`; docs
  live at https://sylphxai.github.io/anymd/.
- Native optional packages are renamed `@sylphx/anymd-<platform>` and ship the
  binary `anymd` (`anymd.exe` on Windows).
- The MCP server identifies itself as `anymd`.
- `ANYMD_RUST_BIN` replaces `CITRA_RUST_BIN` (the old name is still read by the
  SDK) and `ANYMD_NPM_PROVENANCE` replaces `CITRA_NPM_PROVENANCE`.
- The SDK class is `Anymd`; `Citra` stays exported as a deprecated alias.
- `@sylphx/citra` (bin `citra`) and `@sylphx/pdf-reader-mcp` (bin
  `pdf-reader-mcp`) keep working: both are thin alias packages published at the
  same version, depending on `@sylphx/anymd` and running its launcher.
  Existing `npx -y @sylphx/citra` and `npx -y @sylphx/pdf-reader-mcp` configs
  keep working; new installs should use `npx -y @sylphx/anymd`.
