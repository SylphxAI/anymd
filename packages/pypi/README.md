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

## Python API

Wheels built from this source also include a thin Python API over this same
binary; earlier CLI-only wheel releases do not provide these imports. No second
converter or runtime model downloads are involved.

```python
from anymd import convert

document = convert("report.pdf", pages="1-3")
print(document.text)
print(document.metadata["source"])
```

The base API needs only the standard library (Python 3.8+). It converts one local
file or an explicit HTTP(S) URL synchronously. Defaults disable automatic OCR
and embedded-image writes; native page markers and metadata are preserved.
Optional adapters load one framework document without splitting or indexing:

```bash
pip install "anymd[langchain]"    # optional langchain-core
pip install "anymd[llamaindex]"   # optional llama-index-core
```

```python
from anymd.langchain import AnyMDLoader
from anymd.llamaindex import AnyMDReader

langchain_documents = AnyMDLoader("report.pdf").load()
llamaindex_documents = AnyMDReader().load_data("report.pdf")
```

Neither framework is imported by `import anymd`. See the
[Python guide](https://sylphxai.github.io/anymd/guide/python) for options, binary
selection, errors, async limits and metadata handling. Framework versions may
require a newer Python than the base API.

Documentation, benchmarks and source: https://github.com/SylphxAI/anymd. MIT licence.
