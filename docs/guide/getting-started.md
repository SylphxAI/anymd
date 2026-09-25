# Getting started

anymd is one binary that runs as an MCP server for your agent and as a command-line converter. Every MCP client runs the same command:

```bash
npx -y @sylphx/anymd
```

Node 18+ is the only requirement; npm installs the native binary for your platform. No API key, no account.

## Claude Code

```bash
claude mcp add anymd -- npx -y @sylphx/anymd
```

## Codex

```bash
codex mcp add anymd -- npx -y @sylphx/anymd
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.anymd]
command = "npx"
args = ["-y", "@sylphx/anymd"]
```

## Cursor

[Add to Cursor](https://cursor.com/en/install-mcp?name=anymd&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsIkBzeWxwaHgvYW55bWQiXX0=) with one click, or in `.cursor/mcp.json`:

```json
{ "mcpServers": { "anymd": { "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```

## VS Code

```bash
code --add-mcp '{"name":"anymd","command":"npx","args":["-y","@sylphx/anymd"]}'
```

or in `.vscode/mcp.json`:

```json
{ "servers": { "anymd": { "type": "stdio", "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```

## Claude Desktop

Add to `claude_desktop_config.json` (Settings → Developer → Edit Config):

```json
{ "mcpServers": { "anymd": { "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```

## Windsurf, Zed, Cline, and other clients

Any client that speaks MCP over stdio: command `npx`, args `["-y", "@sylphx/anymd"]`.

To keep the server inside one folder, add `--allow-dir`:

```json
{ "command": "npx", "args": ["-y", "@sylphx/anymd", "--allow-dir=/path/to/docs"] }
```

## CLI only

```bash
npm install -g @sylphx/anymd     # or run it once with: npx -y @sylphx/anymd <file>
anymd report.pdf > report.md
```

## Try it

Ask your agent something that needs a document:

> Summarize the results table in `papers/attention.pdf` and cite the page.

The agent calls [`read`](./tools#read) and gets Markdown back with `<!-- page N -->` anchors, so it can cite the page. For a folder of files, it calls [`search`](./tools#search) first.

Run `anymd doctor` to see which optional tools (OCR, audio/video) anymd found on your machine. See [Formats](./formats#optional-tools).
