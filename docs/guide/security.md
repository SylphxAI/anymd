# Security

## Local-first

anymd runs on your machine. Documents never leave it unless you pass a URL, and even then only that URL is fetched. No account or API key is involved.

## URL fetches

When `read` or `search` is given a URL:

- Private, loopback, and link-local addresses are blocked, so a prompt cannot point anymd at your router, cloud metadata endpoint, or local services.
- Every redirect hop is checked again and pinned to the address it resolved to, so DNS rebinding cannot swap in a private address between the check and the connection.
- Downloads are capped at 256 MB.

## Confining file access

By default the server can read any file your user can. To limit it to specific folders, pass `--allow-dir` (repeatable) or set `MCP_PDF_ALLOWED_DIRS`:

```bash
claude mcp add anymd -- npx -y @sylphx/anymd --allow-dir=$HOME/docs --allow-dir=$HOME/papers
```

```json
{
  "command": "npx",
  "args": ["-y", "@sylphx/anymd"],
  "env": { "MCP_PDF_ALLOWED_DIRS": "/home/me/docs" }
}
```

Paths outside the allowed directories, including ones reached through symlinks, are refused.

## External tools

`tesseract`, `ffprobe`, `ffmpeg`, and whisper.cpp are optional. When anymd uses them, it runs them directly, without a shell, with a timeout and an output cap. Nothing from a document is ever interpreted as a command.

## Parsing untrusted files

Every format is parsed in Rust. PDF pages are processed in isolation, so one malformed page cannot take down the whole document, and resource limits bound page count, rendered pixels, and output size.

## Reporting a vulnerability

See [SECURITY.md](https://github.com/SylphxAI/anymd/blob/main/SECURITY.md) for how to report a vulnerability privately.
