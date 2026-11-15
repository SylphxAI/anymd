import importlib.util
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parent / "check-native-cpu.py"
SPEC = importlib.util.spec_from_file_location("check_native_cpu", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
check_native_cpu = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_native_cpu)


def write_cache(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


PORTABLE_CACHE = """// CMakeCache fixture
GGML_NATIVE:BOOL=OFF
TRANSCRIBE_X86_CONSERVATIVE:BOOL=ON
GGML_AVX2:BOOL=OFF
GGML_AVX512:BOOL=OFF
"""


class CheckNativeCpuTests(unittest.TestCase):
    def test_accepts_legacy_and_nested_cargo_build_layouts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_cache(
                root / "transcribe-cpp-sys-0123456789abcdef/out/build/CMakeCache.txt",
                PORTABLE_CACHE,
            )
            self.assertEqual(check_native_cpu.main([str(root)]), 0)

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_cache(
                root / "transcribe-cpp-sys/139ac18434b64481/out/build/CMakeCache.txt",
                PORTABLE_CACHE,
            )
            self.assertEqual(check_native_cpu.main([str(root)]), 0)

    def test_fails_closed_when_cache_missing_or_unreadable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertEqual(check_native_cpu.main([str(root)]), 1)

            bad = root / "transcribe-cpp-sys/deadbeef/out/build/CMakeCache.txt"
            bad.parent.mkdir(parents=True, exist_ok=True)
            bad.write_bytes(b"\xff\xfeGGML_NATIVE:BOOL=OFF\n")
            self.assertEqual(check_native_cpu.main([str(root)]), 1)

    def test_fails_closed_on_native_or_simd_flags(self) -> None:
        bad_configs = [
            "GGML_NATIVE:BOOL=ON\nTRANSCRIBE_X86_CONSERVATIVE:BOOL=ON\n",
            "GGML_NATIVE:BOOL=OFF\nTRANSCRIBE_X86_CONSERVATIVE:BOOL=OFF\n",
            "GGML_NATIVE:BOOL=OFF\nTRANSCRIBE_X86_CONSERVATIVE:BOOL=ON\nGGML_AVX2:BOOL=ON\n",
        ]
        for body in bad_configs:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                write_cache(
                    root / "transcribe-cpp-sys/139ac18434b64481/out/build/CMakeCache.txt",
                    body,
                )
                self.assertEqual(check_native_cpu.main([str(root)]), 1)


if __name__ == "__main__":
    unittest.main()
