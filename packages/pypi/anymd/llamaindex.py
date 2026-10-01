"""Optional LlamaIndex adapter. Importing anymd never imports LlamaIndex."""

from __future__ import annotations

from typing import List, Optional

try:
    from llama_index.core.readers.base import BaseReader
    from llama_index.core.schema import Document
except ImportError as error:
    raise ImportError(
        'AnyMDReader requires the optional dependency: pip install "anymd[llamaindex]"'
    ) from error

from . import convert
from ._api import Source


class AnyMDReader(BaseReader):
    """Read one file or URL into one LlamaIndex document."""

    def __init__(self, **convert_options):
        self.convert_options = convert_options

    def load_data(
        self, file: Source, extra_info: Optional[dict] = None
    ) -> List[Document]:
        document = convert(file, **self.convert_options)
        metadata = dict(extra_info or {})
        # Native provenance wins over caller-supplied conflicting keys.
        metadata.update(document.metadata)
        return [Document(text=document.text, metadata=metadata)]
