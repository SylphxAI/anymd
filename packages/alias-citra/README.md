# @sylphx/citra → [@sylphx/anymd](https://www.npmjs.com/package/@sylphx/anymd)

**Citra is now [anymd](https://github.com/SylphxAI/anymd)** — any file → clean
Markdown for AI agents: PDF, Word, PowerPoint, Excel, EPUB, HTML, images. Fast
Rust MCP server + CLI. Local, no API key.

This package is a thin alias kept so existing install commands keep working.
It depends on `@sylphx/anymd` at the exact same version and runs its launcher
with the same arguments and stdio, so `citra` behaves exactly like `anymd`.

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

Docs: https://sylphxai.github.io/anymd/
