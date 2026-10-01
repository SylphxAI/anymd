"""Standard-library tests; real native tests use ANYMD_BIN when available."""

import importlib.util
from importlib.metadata import PackageNotFoundError
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from anymd import (  # noqa: E402
    BinaryNotFoundError,
    ConversionError,
    ConversionTimeoutError,
    convert,
)

ROOT = Path(__file__).resolve().parents[3]
OUTPUT = b'---\nsource: "a: b.pdf"\ntitle: "A \\"quoted\\" title"\npages: 2\n---\n\n<!-- page 1 -->\nHello\n'


class ApiTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.source = Path(self.directory.name) / "--help.txt"
        self.source.write_text("Hello", encoding="utf-8")

    @patch("anymd._api.subprocess.run")
    def test_defaults_preserve_body_and_metadata(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, OUTPUT, b"")
        doc = convert(self.source, binary="native")
        self.assertEqual(doc.text, "<!-- page 1 -->\nHello\n")
        self.assertEqual(doc.source, "a: b.pdf")
        self.assertEqual(
            doc.metadata,
            {"source": "a: b.pdf", "title": 'A "quoted" title', "pages": "2"},
        )
        args, kwargs = run.call_args
        self.assertEqual(
            args[0],
            [
                "native",
                str(self.source),
                "--front-matter",
                "--images",
                "none",
                "--revisions",
                "markup",
                "--no-ocr",
            ],
        )
        self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
        self.assertNotIn("shell", kwargs)
        self.assertEqual(kwargs["timeout"], 120.0)

    @patch("anymd._api.subprocess.run")
    def test_options_and_explicit_url(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, OUTPUT, b"")
        convert(
            "https://example.com/a.pdf",
            binary="native",
            pages="1-3",
            node="heading",
            max_tokens=20,
            cursor="--help",
            ocr=True,
            transcript=True,
            images="refs",
            revisions="accept",
            timeout=2,
        )
        command = run.call_args[0][0]
        for option in (
            "--pages=1-3",
            "--node=heading",
            "--max-tokens=20",
            "--cursor=--help",
            "--ocr",
            "--transcript",
        ):
            self.assertIn(option, command)
        self.assertNotIn("--download-whisper-model", command)
        convert(self.source, binary="native", ocr=None)
        self.assertNotIn("--no-ocr", run.call_args[0][0])
        self.assertNotIn("--ocr", run.call_args[0][0])

    @patch("anymd._api.subprocess.run")
    def test_bad_inputs_never_run_binary(self, run):
        for source in (
            "",
            "-",
            "mcp",
            Path(self.directory.name),
            " https://example.com/a",
        ):
            with self.subTest(source=source), self.assertRaises(ValueError):
                convert(source)
        for options in (
            {"images": "all"},
            {"revisions": "all"},
            {"ocr": 1},
            {"transcript": 1},
            {"max_tokens": 0},
            {"max_tokens": True},
            {"max_tokens": 2**32},
            {"timeout": 0},
            {"timeout": float("inf")},
            {"timeout": float("nan")},
            {"pages": ""},
            {"cursor": 3},
            {"node": ""},
        ):
            with self.subTest(options=options), self.assertRaises(ValueError):
                convert(self.source, binary="native", **options)
        run.assert_not_called()

    @patch("anymd._api.subprocess.run")
    def test_native_failure_and_timeout(self, run):
        run.return_value = subprocess.CompletedProcess(
            [], 2, b"partial output", b"bad page\n"
        )
        with self.assertRaises(ConversionError) as caught:
            convert(self.source, binary="native")
        self.assertEqual(caught.exception.returncode, 2)
        self.assertEqual(caught.exception.stderr, "bad page\n")
        run.side_effect = subprocess.TimeoutExpired("native", 1)
        with self.assertRaises(ConversionTimeoutError):
            convert(self.source, binary="native", timeout=1)
        run.side_effect = FileNotFoundError("missing")
        with self.assertRaises(BinaryNotFoundError):
            convert(self.source, binary="missing")

    @patch("anymd._api.subprocess.run")
    def test_bad_output_is_not_a_document(self, run):
        for output in (
            b"no header",
            b"---\nsource: x\n",
            b"---\ntitle: x\n---\nbody",
            b'---\nsource: "bad\n---\nbody',
            b'---\nsource: ""\n---\nbody',
            b"---\ninvalid\n---\nbody",
            b"\xff",
        ):
            run.return_value = subprocess.CompletedProcess([], 0, output, b"")
            with self.subTest(output=output), self.assertRaises(ConversionError):
                convert(self.source, binary="native")

    @patch("anymd._api.metadata.distribution", side_effect=PackageNotFoundError)
    @patch("anymd._api.subprocess.run")
    def test_binary_selection(self, run, distribution):
        run.return_value = subprocess.CompletedProcess([], 0, OUTPUT, b"")
        with patch.dict(os.environ, {"ANYMD_BIN": "from-env"}):
            convert(self.source)
            self.assertEqual(run.call_args[0][0][0], "from-env")
            convert(self.source, binary="explicit")
            self.assertEqual(run.call_args[0][0][0], "explicit")
        with patch.dict(os.environ, {"ANYMD_BIN": ""}), self.assertRaises(BinaryNotFoundError):
            convert(self.source)
        with patch.dict(os.environ, {}, clear=True), patch(
            "anymd._api.sysconfig.get_path", return_value=self.directory.name
        ):
            installed = Path(self.directory.name) / (
                "anymd.exe" if os.name == "nt" else "anymd"
            )
            installed.touch()
            convert(self.source)
            self.assertEqual(run.call_args[0][0][0], str(installed))
            installed.unlink()
            with patch("anymd._api.shutil.which", return_value="from-path"):
                convert(self.source)
                self.assertEqual(run.call_args[0][0][0], "from-path")
            with patch("anymd._api.shutil.which", return_value=None), self.assertRaises(BinaryNotFoundError):
                convert(self.source)

    @patch("anymd._api.subprocess.run")
    def test_wheel_record_precedes_fallback_and_missing_binary_fails(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, OUTPUT, b"")
        exe = "anymd.exe" if os.name == "nt" else "anymd"
        installed = Path(self.directory.name) / exe
        installed.touch()
        distribution = Mock(files=[Path("../../../bin") / exe])
        distribution.locate_file.return_value = installed
        with patch.dict(os.environ, {}, clear=True), patch(
            "anymd._api.metadata.distribution", return_value=distribution
        ), patch("anymd._api.shutil.which", return_value="competing") as which:
            convert(self.source)
            self.assertEqual(run.call_args[0][0][0], str(installed))
            which.assert_not_called()
            for files in (distribution.files, [], None):
                installed.unlink(missing_ok=True)
                distribution.files = files
                with self.assertRaises(BinaryNotFoundError):
                    convert(self.source)
                which.assert_not_called()

    def test_core_import_never_imports_frameworks(self):
        subprocess.run(
            [
                sys.executable,
                "-c",
                "import sys, anymd; assert not any(x.startswith(('langchain', 'llama_index')) for x in sys.modules)",
            ],
            env=dict(os.environ, PYTHONPATH=str(ROOT / "packages/pypi")),
            check=True,
        )

    def test_missing_framework_errors(self):
        code = """
import sys
class BlockFrameworks:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.startswith(('langchain_core', 'llama_index')):
            raise ImportError('blocked for test')
sys.meta_path.insert(0, BlockFrameworks())
for module, extra in [('anymd.langchain', 'langchain'), ('anymd.llamaindex', 'llamaindex')]:
    try:
        __import__(module)
    except ImportError as error:
        assert 'anymd[' + extra + ']' in str(error)
    else:
        raise AssertionError('expected optional dependency error')
"""
        subprocess.run(
            [sys.executable, "-c", code],
            env=dict(os.environ, PYTHONPATH=str(ROOT / "packages/pypi")),
            check=True,
        )


@unittest.skipUnless(
    os.environ.get("ANYMD_BIN"),
    "set ANYMD_BIN to a native binary for lightweight fixture tests",
)
class NativeTests(unittest.TestCase):
    def test_pdf_provenance_and_selection(self):
        doc = convert(ROOT / "test/fixtures/sample.pdf", pages="1")
        self.assertIn("<!-- page 1 -->", doc.text)
        self.assertEqual(doc.source, str(ROOT / "test/fixtures/sample.pdf"))
        self.assertIn("pages", doc.metadata)

    def test_csv_matches_native_body(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "data.csv"
            source.write_text("name,value\napple,2\n", encoding="utf-8")
            doc = convert(source)
            native = subprocess.run(
                [os.environ["ANYMD_BIN"], str(source), "--no-ocr", "--images", "none"],
                stdout=subprocess.PIPE,
                check=True,
            ).stdout.decode("utf-8")
            self.assertEqual(doc.text, native)
            self.assertEqual(doc.metadata["format"], "csv")


class FrameworkTests(unittest.TestCase):
    @unittest.skipUnless(
        importlib.util.find_spec("langchain_core")
        and importlib.util.find_spec("llama_index")
        and os.environ.get("ANYMD_BIN"),
        "install optional frameworks and set ANYMD_BIN",
    )
    def test_adapters_do_not_request_network_or_downloads(self):
        code = """
import socket
from unittest.mock import patch
with patch.object(socket.socket, 'connect', side_effect=AssertionError('unexpected runtime network')):
    from anymd.langchain import AnyMDLoader
    from anymd.llamaindex import AnyMDReader
    import sys
    assert AnyMDLoader(sys.argv[1]).load()[0].page_content
    assert AnyMDReader().load_data(sys.argv[1])[0].text
"""
        subprocess.run(
            [sys.executable, "-c", code, str(ROOT / "test/fixtures/sample.pdf")],
            env=dict(os.environ, PYTHONPATH=str(ROOT / "packages/pypi")),
            check=True,
        )

    @unittest.skipUnless(
        importlib.util.find_spec("langchain_core"), "optional LangChain not installed"
    )
    def test_langchain_loader(self):
        from anymd import Document
        from anymd.langchain import AnyMDLoader
        from langchain_core.document_loaders import BaseLoader
        from langchain_core.documents import Document as LCDocument

        with patch(
            "anymd.langchain.convert",
            return_value=Document("body\n", {"source": "file", "pages": "1"}),
        ) as call:
            loader = AnyMDLoader("file", pages="1")
            self.assertIsInstance(loader, BaseLoader)
            documents = loader.load()
            self.assertIsInstance(documents[0], LCDocument)
            self.assertEqual(documents[0].page_content, "body\n")
            self.assertEqual(documents[0].metadata, {"source": "file", "pages": "1"})
            call.assert_called_once_with("file", pages="1")
            import asyncio

            self.assertEqual(asyncio.run(loader.aload())[0].page_content, "body\n")

    @unittest.skipUnless(
        importlib.util.find_spec("llama_index"), "optional LlamaIndex not installed"
    )
    def test_llamaindex_reader(self):
        from anymd import Document
        from anymd.llamaindex import AnyMDReader
        from llama_index.core.readers.base import BaseReader
        from llama_index.core.schema import Document as LIDocument

        with patch(
            "anymd.llamaindex.convert",
            return_value=Document("body\n", {"source": "file", "title": "Title"}),
        ) as call:
            reader = AnyMDReader(images="none")
            self.assertIsInstance(reader, BaseReader)
            documents = reader.load_data(
                "file", extra_info={"source": "wrong", "team": "docs"}
            )
            self.assertIsInstance(documents[0], LIDocument)
            self.assertEqual(documents[0].text, "body\n")
            self.assertEqual(
                documents[0].metadata,
                {"source": "file", "title": "Title", "team": "docs"},
            )
            call.assert_called_once_with("file", images="none")


if __name__ == "__main__":
    unittest.main()
