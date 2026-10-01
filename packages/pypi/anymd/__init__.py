"""Thin, local Python interface to the installed anymd CLI."""

from ._api import (
    AnyMDError,
    BinaryNotFoundError,
    ConversionError,
    ConversionTimeoutError,
    Document,
    convert,
)

__all__ = [
    "AnyMDError",
    "BinaryNotFoundError",
    "ConversionError",
    "ConversionTimeoutError",
    "Document",
    "convert",
]
