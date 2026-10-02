---
description: "Install anymd in Claude Code, Codex, Cursor, VS Code, Claude Desktop and other MCP clients with one command, or use it as a CLI."
---

# Getting started

anymd is one binary that runs as an MCP server for your agent and as a command-line converter. Every MCP client runs the same command:

```bash
npx -y @sylphx/anymd
```

Node 18+ is the only requirement; npm installs the native binary for your platform. No API key, no account.

To add anymd to every MCP client on your machine at once (Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf, Gemini CLI):

```bash
npx -y @sylphx/anymd setup     # --dry-run to preview, --remove to undo
```

It is safe to run again. The sections below add it to one client by hand.

Prefer Python or Docker? These run the same prebuilt binary, with no Node needed:

```bash
uvx anymd report.pdf > report.md         # run once; or: pip install anymd
uvx anymd mcp                            # the MCP server, for clients that take a command
docker run --rm -v "$PWD:/data" ghcr.io/sylphxai/anymd report.pdf > report.md
docker run -i --rm ghcr.io/sylphxai/anymd    # MCP server on stdio (amd64 and arm64)
```

The Linux wheels and the image use glibc 2.17 or newer, so Alpine (musl) needs `cargo install anymd` instead.

Prefer Cargo? `cargo install anymd` builds the same binary from [crates.io](https://crates.io/crates/anymd). It needs Rust 1.95+, CMake and a C++ compiler, and includes local doc-VLM OCR and bundled ASR. Run `anymd setup ocr` to install the pinned models explicitly; without setup, automatic OCR keeps using installed `tesseract`. Audio and video still use optional `ffmpeg`.

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

[Install in VS Code](https://insiders.vscode.dev/redirect?url=vscode%3Amcp%2Finstall%3F%257B%2522name%2522%253A%2522anymd%2522%252C%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522%2540sylphx%252Fanymd%2522%255D%257D) with one click, or from a terminal:

```bash
code --add-mcp '{"name":"anymd","command":"npx","args":["-y","@sylphx/anymd"]}'
```

or in `.vscode/mcp.json`:

```json
{ "servers": { "anymd": { "type": "stdio", "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```

## Claude Desktop

One click: download `anymd-<version>.mcpb` from the [latest release](https://github.com/SylphxAI/anymd/releases/latest) and open it in Claude Desktop. `anymd-<version>-<platform>.mcpb` carries one platform's binary and is smaller. Or by hand:

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
# or: uvx anymd <file> · pip install anymd · docker run ghcr.io/sylphxai/anymd <file>
anymd report.pdf > report.md
```

## Verify a download

Every release asset (the `.tar.gz` and `.zip` binaries and the `.mcpb` bundles) is listed in `SHA256SUMS` and carries a signed [GitHub artifact attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds) that names the workflow run that built it. The release also attaches a CycloneDX software bill of materials (`anymd-<version>.cdx.json`).

```bash
# download an asset and SHA256SUMS from the release, then:
sha256sum -c SHA256SUMS --ignore-missing        # macOS: shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify anymd-darwin-arm64.tar.gz --repo SylphxAI/anymd
```

## Try it

Ask your agent something that needs a document:

> Summarize the results table in `papers/attention.pdf` and cite the page.

The agent calls [`read`](./tools#read) and gets Markdown back with `<!-- page N -->` anchors, so it can cite the page. For a folder of files, it calls [`search`](./tools#search) first.

Run `anymd doctor` to see which optional tools (OCR, audio/video) anymd found on your machine. See [Formats](./formats#optional-tools).
