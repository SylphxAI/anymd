"""Optional LangChain adapter. Importing anymd itself never imports LangChain."""

from __future__ import annotations

from typing import Iterator

try:
    from langchain_core.document_loaders import BaseLoader
    from langchain_core.documents import Document
except ImportError as error:
    raise ImportError(
        'AnyMDLoader requires the optional dependency: pip install "anymd[langchain]"'
    ) from error

from . import convert
from ._api import Source


class AnyMDLoader(BaseLoader):
    """Load one native conversion as one LangChain document, without splitting."""

    def __init__(self, source: Source, **convert_options):
        self.source = source
        self.convert_options = convert_options

    def lazy_load(self) -> Iterator[Document]:
        document = convert(self.source, **self.convert_options)
        yield Document(page_content=document.text, metadata=dict(document.metadata))
