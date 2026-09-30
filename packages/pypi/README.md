# anymd

Any file to clean Markdown for AI agents: PDF, Word, PowerPoint, Excel, EPUB, HTML and web pages, images (OCR), audio and video. A fast Rust CLI and MCP server that runs on your machine. No API key.

This package carries the native `anymd` binary for your platform, so there is nothing else to install.

```bash
uvx anymd report.pdf > report.md     # run once, no install
pip install anymd                    # or install it
anymd report.pdf > report.md
```

Run it as an MCP server (stdio):

```bash
uvx anymd mcp
```

Add it to every MCP client on your machine with `uvx anymd setup`.

Documentation, benchmarks and source: https://github.com/SylphxAI/anymd. MIT licence.
