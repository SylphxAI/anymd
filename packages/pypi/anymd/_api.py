"""The native CLI owns conversion; this module only marshals its inputs/output."""

from __future__ import annotations

import json
import math
import os
import shutil
import subprocess
import sysconfig
from dataclasses import dataclass
from importlib import metadata
from pathlib import Path
from typing import Dict, Optional, Union

Source = Union[str, os.PathLike]


class AnyMDError(RuntimeError):
    """Base class for native execution and output errors."""


class BinaryNotFoundError(AnyMDError):
    """The selected native executable is absent or cannot be started."""


class ConversionError(AnyMDError):
    """The CLI failed or returned an unexpected output format."""

    def __init__(
        self, message: str, *, returncode: Optional[int] = None, stderr: str = ""
    ):
        super().__init__(message)
        self.returncode = returncode
        self.stderr = stderr


class ConversionTimeoutError(AnyMDError):
    """The native process exceeded the caller's timeout and was stopped."""


@dataclass
class Document:
    """Markdown body and the CLI's flat, string-valued source metadata."""

    text: str
    metadata: Dict[str, str]

    @property
    def source(self) -> str:
        return self.metadata["source"]


def _binary(binary: Optional[Source]) -> str:
    selected = os.fspath(binary) if binary is not None else os.environ.get("ANYMD_BIN")
    if selected is not None:
        if not selected:
            raise BinaryNotFoundError("anymd binary path is empty")
        return selected
    exe = "anymd.exe" if os.name == "nt" else "anymd"
    # Installed RECORD paths know where pip put this wheel's script, including
    # --user/PYTHONUSERBASE installs outside this interpreter's default scripts.
    try:
        distribution = metadata.distribution("anymd")
    except metadata.PackageNotFoundError:
        distribution = None
    if distribution is not None:
        for entry in distribution.files or ():
            if entry.name == exe:
                installed = Path(distribution.locate_file(entry))
                if installed.is_file():
                    return str(installed)
        raise BinaryNotFoundError(
            "The installed anymd wheel has no native binary; reinstall it or set ANYMD_BIN"
        )
    # Source checkouts and other CLI installs may have no wheel metadata.
    installed = Path(sysconfig.get_path("scripts")) / exe
    if installed.is_file():
        return str(installed)
    found = shutil.which(exe)
    if found:
        return found
    raise BinaryNotFoundError(
        "Install the anymd platform wheel or set ANYMD_BIN to its native binary"
    )


def _document(output: str) -> Document:
    # Native front matter is a flat map, not arbitrary YAML. Quoted values are
    # JSON strings (lean.rs::yaml_value); keep numbers and dates as strings.
    lines = output.split("\n")
    if not lines or lines[0] != "---":
        raise ConversionError(
            "anymd returned no source header; check the native binary version"
        )
    metadata: Dict[str, str] = {}
    for index, line in enumerate(lines[1:], 1):
        if line == "---":
            body = "\n".join(lines[index + 1 :])
            if body.startswith("\n"):
                body = body[1:]
            if not metadata.get("source"):
                raise ConversionError("anymd returned no source metadata")
            return Document(body, metadata)
        key, separator, value = line.partition(": ")
        if not separator or not key:
            raise ConversionError("anymd returned an invalid source header")
        if value.startswith('"'):
            try:
                value = json.loads(value)
            except (ValueError, TypeError) as error:
                raise ConversionError(
                    "anymd returned invalid quoted metadata"
                ) from error
            if not isinstance(value, str):
                raise ConversionError("anymd returned non-string metadata")
        metadata[key] = value
    raise ConversionError("anymd returned an unfinished source header")


def convert(
    source: Source,
    *,
    pages: Optional[str] = None,
    node: Optional[str] = None,
    max_tokens: Optional[int] = None,
    cursor: Optional[str] = None,
    ocr: Optional[bool] = False,
    transcript: bool = False,
    images: str = "none",
    revisions: str = "markup",
    timeout: float = 120.0,
    binary: Optional[Source] = None,
) -> Document:
    """Convert one file or explicit HTTP(S) URL, without a shell or downloads.

    Defaults disable automatic OCR and embedded-image writes. Set ``ocr=None``
    for the CLI's automatic OCR behavior, or ``ocr=True`` to request OCR.
    ``transcript=True`` uses bundled Qwen3-ASR with preinstalled pinned weights;
    it never implicitly downloads a model.
    Cursor notes and page/source markers remain in the Markdown body.
    """
    value = os.fspath(source)
    if not isinstance(value, str) or not value or value != value.strip():
        raise ValueError(
            "source must be a nonempty text path or HTTP(S) URL with no surrounding whitespace"
        )
    if not value.startswith(("http://", "https://")):
        path = Path(value).resolve()
        if not path.is_file():
            raise ValueError(
                "source must name an existing file, not a directory or stdin"
            )
        # Absolute paths cannot be mistaken for CLI subcommands or flags.
        value = str(path)
    if images not in ("none", "refs"):
        raise ValueError("images must be none or refs")
    if revisions not in ("markup", "accept", "reject"):
        raise ValueError("revisions must be markup, accept or reject")
    if ocr is not None and not isinstance(ocr, bool):
        raise ValueError("ocr must be True, False or None")
    if not isinstance(transcript, bool):
        raise ValueError("transcript must be a bool")
    if max_tokens is not None and (
        isinstance(max_tokens, bool)
        or not isinstance(max_tokens, int)
        or not 1 <= max_tokens <= 4294967295
    ):
        raise ValueError("max_tokens must be a positive 32-bit integer")
    if (
        isinstance(timeout, bool)
        or not isinstance(timeout, (int, float))
        or not math.isfinite(timeout)
        or timeout <= 0
    ):
        raise ValueError("timeout must be a finite positive number of seconds")
    command = [
        _binary(binary),
        value,
        "--front-matter",
        "--images",
        images,
        "--revisions",
        revisions,
    ]
    for flag, option in (
        ("--pages", pages),
        ("--node", node),
        ("--max-tokens", max_tokens),
        ("--cursor", cursor),
    ):
        if option is not None:
            if flag != "--max-tokens" and (not isinstance(option, str) or not option):
                raise ValueError(flag + " must be a nonempty string")
            # Inline values keep even a leading dash from becoming another flag.
            command.append(flag + "=" + str(option))
    if ocr is not None:
        command.append("--ocr" if ocr else "--no-ocr")
    if transcript:
        command.append("--transcript")
    try:
        result = subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ConversionTimeoutError(
            "anymd conversion exceeded {} seconds".format(timeout)
        ) from error
    except OSError as error:
        raise BinaryNotFoundError(
            "Cannot start anymd binary: {}".format(error)
        ) from error
    stderr = result.stderr.decode("utf-8", errors="replace")
    if result.returncode:
        raise ConversionError(
            stderr.strip() or "anymd conversion failed",
            returncode=result.returncode,
            stderr=stderr,
        )
    try:
        output = result.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ConversionError("anymd returned non-UTF-8 Markdown") from error
    return _document(output)
