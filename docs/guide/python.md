# Python API and document loaders

The platform wheel contains the native CLI and a thin Python API. Conversion
runs in the same Rust binary as the CLI; Python does not implement a converter.
The base package uses only the Python standard library (Python 3.8 or newer).

anymd 8.3.0 platform wheels include this API. Older CLI-only wheels do not
provide these imports.

## Convert one document

```bash
pip install anymd
```

```python
from anymd import convert

document = convert("report.pdf", pages="1-3")
print(document.text)
print(document.source)
print(document.metadata)
```

`convert(source, **options)` returns a `Document` with:

- `text`: the native Markdown body, unchanged, including page markers, image
  references when requested, and continuation notes when a budget is set.
- `metadata`: every field in the CLI's source header, as strings. Depending on
  format, these can include `source`, `title`, `format`, `pages`, `showing` and
  document properties. Missing fields are not invented.
- `source`: the header's source value, also present in `metadata`.

The source is one existing local file or an explicit `http://` or `https://`
URL. Local paths become absolute paths, so filenames such as `--help.txt` cannot
be read as CLI options. Directories, stdin, and multiple inputs are not part of
this API. Use the CLI or MCP tools for directory listing and search.

### Options and defaults

| Python keyword | Default | Native CLI option |
| --- | --- | --- |
| `pages` | `None` | `--pages` (pages, slides, sheets or chapters) |
| `node` | `None` | `--node` (ID from a native outline) |
| `max_tokens` | `None` | `--max-tokens` (positive 32-bit integer) |
| `cursor` | `None` | `--cursor` |
| `ocr` | `False` | `--no-ocr`; `True` passes `--ocr`; `None` uses native automatic OCR |
| `transcript` | `False` | `--transcript` |
| `images` | `"none"` | `--images none`; `"refs"` writes images to the native cache |
| `revisions` | `"markup"` | `--revisions markup`, `accept` or `reject` |
| `timeout` | `120.0` | Process timeout in seconds; must be finite and positive |
| `binary` | `None` | Override the native executable path |

The defaults deliberately disable automatic OCR and embedded-image writes, so
optional tools installed on a machine do not silently change the request.
Conversion accuracy and output still depend on the native version and source.
Pages, nodes and cursors are validated by the CLI. The wrapper always requests
front matter and separates it from the Markdown body; it does not parse arbitrary
YAML inside the document. There is no output-file option: write `document.text`
yourself when needed.

The Python API never requests model or executable downloads. Transcription uses
bundled Qwen3-ASR with cached or preinstalled pinned weights; install the ASR model
explicitly with the native CLI’s `--download-asr-model` option before requesting
`transcript=True`. For doc-VLM OCR, run `anymd setup ocr` explicitly; `ocr=True`
inherits the native engine default, while `ocr=False` keeps OCR disabled.
Requesting OCR or transcription without its native
requirements produces the CLI's error. Passing a URL explicitly requests a
network fetch under the native URL policy; local-file conversion never requests
one through this wrapper.

Executable selection is: explicit `binary`, `ANYMD_BIN`, then the native script
recorded in the installed `anymd` wheel's metadata. This includes `pip --user`
and custom `PYTHONUSERBASE` installations, even without their scripts on `PATH`.
Only when wheel metadata is absent does selection fall back to the current
Python installation's scripts directory, then `PATH`. A wheel missing its
recorded binary or an invalid explicit override fails instead of selecting a
competing installation. No shell is used. `convert` is synchronous; there is no
separate Python async converter or MCP client.

### Errors

```python
from anymd import AnyMDError, ConversionError, convert

try:
    document = convert("report.pdf", timeout=30)
except ConversionError as error:
    print(error.returncode, error.stderr)
except AnyMDError as error:
    print(str(error))
```

- Invalid wrapper inputs raise `ValueError` before execution.
- `BinaryNotFoundError`: the executable is absent or cannot be started.
- `ConversionTimeoutError`: the process timed out and was stopped.
- `ConversionError`: a nonzero native exit status, invalid UTF-8, or unexpected
  source header. Native failures expose `returncode` and `stderr`; malformed
  output has `returncode=None`. Failed or partial output is not returned as a
  successful document.

## LangChain loader

Install the optional core dependency; no LLM provider, embedding model or vector
store is needed. Framework releases may require a newer Python than the base API.

```bash
pip install "anymd[langchain]"
```

```python
from anymd.langchain import AnyMDLoader

loader = AnyMDLoader("report.pdf", pages="1-3", timeout=30)
documents = loader.load()
# Or: for document in loader.lazy_load(): ...
print(documents[0].page_content)
print(documents[0].metadata["source"])
```

One source becomes one LangChain `Document`. Native Markdown and metadata are
preserved; there is no automatic splitting, indexing or model invocation.
`AnyMDLoader` extends `langchain_core`'s `BaseLoader`. Its inherited `aload` and
`alazy_load` methods use LangChain's thread-backed sync loader, not an async
native conversion protocol. The conversion timeout still applies.

## LlamaIndex reader

```bash
pip install "anymd[llamaindex]"
```

```python
from anymd.llamaindex import AnyMDReader

reader = AnyMDReader(pages="1-3", timeout=30)
documents = reader.load_data("report.pdf", extra_info={"collection": "reports"})
print(documents[0].text)
print(documents[0].metadata["source"])
```

`AnyMDReader` extends `llama_index.core`'s `BaseReader` and returns one LlamaIndex
`Document` per `load_data` call. It accepts `file` as a positional or keyword
argument and merges optional `extra_info`. Native metadata wins on conflicting
keys, so caller labels cannot replace the source provenance. This reader exposes
the synchronous `load_data` interface; it does not implement native async
conversion. On framework versions with inherited `aload_data`, that method
delegates to this sync reader (thread-backed in the tested 0.14 series).

Importing `anymd` never imports either framework. Each adapter imports its
framework only when that adapter module is imported, and gives an install hint
if the dependency is missing. The wheel declares both dependencies as optional
extras, not base requirements.

Runnable, local-only examples are in
[`examples/python`](https://github.com/SylphxAI/anymd/tree/main/examples/python).
They load documents without building indexes or downloading models.
