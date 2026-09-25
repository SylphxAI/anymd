# Installation

One line. No Docker, no API key, no global install.

```bash
npx -y @sylphx/anymd
```

That starts a **stdio MCP server** your agent can use immediately. Prefer `npx`
in host configuration so nothing is installed globally and upgrades are just a
cache refresh.

The package was formerly `@sylphx/pdf-reader-mcp`. New installs use `@sylphx/anymd`.

## Requirements

| | |
| --- | --- |
| Node.js | **≥ 22.13.0** (the thin launcher only — the PDF engine is native) |
| Platforms | macOS arm64 / x64 · Linux x64-gnu / arm64-gnu · Windows x64 |
| Optional OCR / visual providers | opt-in; **not** required for text, tables, or citations |

## Add it to your agent

::: code-group
```json [Claude Desktop / Cursor / VS Code / Codex]
{
  "mcpServers": {
    "anymd": {
      "command": "npx",
      "args": ["-y", "@sylphx/anymd"]
    }
  }
}
```

```bash [Claude Code]
claude mcp add anymd -- npx -y @sylphx/anymd
```

```bash [Any agent or CLI]
npx -y @sylphx/anymd
```
:::

<details>
<summary><strong>Claude Desktop config file locations</strong></summary>

- **macOS** — `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows** — `%APPDATA%\Claude\claude_desktop_config.json`
- **Linux** — `~/.config/Claude/claude_desktop_config.json`

</details>

<details>
<summary><strong>Dual-era clients</strong> (send <code>server/discover</code> before <code>initialize</code>)</summary>

Hosts such as the Gemini Antigravity CLI probe with SEP-2575 `server/discover`
before the legacy handshake. anymd answers both, on stdio and over HTTP.

</details>

## Global CLI

```bash
npm install -g @sylphx/anymd
anymd --help
```

## Pin a version

Any released version can be pinned by its exact number:

```bash
npx -y @sylphx/anymd@<version>
```

## SDK

```ts
import { Anymd } from '@sylphx/anymd/sdk';

const anymd = new Anymd();
const read = await anymd.read({ sources: [{ path: '/absolute/path/report.pdf' }] });
```

`@sylphx/anymd/sdk` exposes `read` / `search` / `evidence` — the same three
surfaces as the MCP tools. `@sylphx/anymd/pure-rust` exposes the low-level
client helpers. Both require the platform native package, exactly like MCP.

## What gets installed

The launcher package ships **zero production JS dependencies**. npm selects
exactly **one** platform native package for your host:

| Platform | Optional native package |
| --- | --- |
| macOS arm64 | `@sylphx/anymd-darwin-arm64` |
| macOS x64 | `@sylphx/anymd-darwin-x64` |
| Linux x64 | `@sylphx/anymd-linux-x64-gnu` |
| Linux arm64 | `@sylphx/anymd-linux-arm64-gnu` |
| Windows x64 | `@sylphx/anymd-win32-x64-msvc` |

Measured clean install (linux-x64): **20 files**, ~**24.4 MiB** of `node_modules`
— versus 4,101 files and ~82.3 MiB for the historical TypeScript engine. The
native binary is multi-megabyte because it *is* the PDF engine.

## Fail closed

There is **no** TypeScript PDF runtime in the production package. If the
matching native binary is missing or the wrong version, the launcher refuses to
start and names the platform it expected. You will never get a silent
downgrade to a different engine.

## Verify

```bash
npx -y @sylphx/anymd --help
```

Then, in your agent, ask:

> Read `/absolute/path/to/sample.pdf` and give me the page number and table cell
> behind your answer.

If the reply carries `page`, `bounding_box`, and `provenance`, you are on the
evidence path. If it quotes text with no locators, you are not talking to anymd.

## Optional providers

Core text, tables and citations need **no** provider and no network. These are
opt-in and configure by environment:

| Purpose | Variables |
| --- | --- |
| OCR (scanned pages) | `MCP_PDF_OCR_PRESET` (`tesseract` / `tesseract-tsv`) or `MCP_PDF_OCR_COMMAND` + `MCP_PDF_OCR_ARGS_JSON` |
| Region / visual analysis | `MCP_PDF_REGION_ANALYSIS_COMMAND`, or `MCP_PDF_REGION_ANALYSIS_PRESET` = `ollama` / `openai-compatible` / `lmstudio` / `llamacpp` |
| HTTP transport | `MCP_TRANSPORT=http`, `MCP_HTTP_HOST`, `MCP_HTTP_PORT`, `MCP_API_KEY` |
| Filesystem reach | `--allow-dir=<path>` (repeatable) or `MCP_PDF_ALLOWED_DIRS` |

## Security defaults

- Local files are read only from paths you pass; `--allow-dir` scopes that down further.
- Remote URLs are guarded: scheme and host policy, an SSRF deny-list pinned to
  the connected address (no DNS-rebinding gap), size caps, and bounded redirects.
- The HTTP transport binds **loopback** by default and refuses a non-loopback
  bind without `MCP_API_KEY`.

Full reference: [API docs](/api/) · [security reporting](/security/maintainer-process)

## Next

→ [Quickstart: from PDF to proof](/guide/getting-started)
