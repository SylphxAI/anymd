# @sylphx/pdf-reader-mcp → [@sylphx/anymd](https://www.npmjs.com/package/@sylphx/anymd)

**pdf-reader-mcp is now [anymd](https://github.com/SylphxAI/anymd)** — any file → clean
Markdown for AI agents: PDF, Word, PowerPoint, Excel, EPUB, HTML, images. Fast
Rust MCP server + CLI. Local, no API key.

This package is a thin alias kept so existing install commands keep working.
It depends on `@sylphx/anymd` at the exact same version and runs its launcher
with the same arguments and stdio, so `pdf-reader-mcp` behaves exactly like `anymd`.

New installs should use the canonical package:

```bash
npx -y @sylphx/anymd
```

MCP client config:

```json
{
  "mcpServers": {
    "anymd": { "command": "npx", "args": ["-y", "@sylphx/anymd"] }
  }
}
```

## anymd Pro ($29 once)

All conversion, search, outline, OCR and transcript tools remain free and MIT licensed. Need deterministic PDF quote verification (`cite_check`) or SHA-256 verifiable video timelines and frame extraction (`video_timeline`, `render_frame`)? [Get Anymd Pro for US$29 once](https://anymd.sylphx.com/pricing?entry_surface=npm_pdf_reader_mcp), then run `anymd login` and sign in with the same account to activate Pro on this machine. The Pro tools work offline after activation.

Docs: https://anymd.sylphx.com/
