#!/usr/bin/env python3
"""Check the actual bundled ASR CMake cache, not merely the requested flags."""
import sys
from pathlib import Path

SIMD = (
    "GGML_SSE42", "GGML_AVX", "GGML_AVX_VNNI", "GGML_AVX2", "GGML_BMI2",
    "GGML_FMA", "GGML_F16C", "GGML_AVX512", "GGML_AVX512_VBMI",
    "GGML_AVX512_VNNI", "GGML_AVX512_BF16",
)


def check(cache: Path) -> None:
    values = {}
    for line in cache.read_text().splitlines():
        if not line or line.startswith(("#", "//")) or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.split(":", 1)[0]] = value
    if values.get("GGML_NATIVE") != "OFF":
        raise ValueError(f"{cache}: GGML_NATIVE must be OFF")
    if values.get("TRANSCRIBE_X86_CONSERVATIVE") != "ON":
        raise ValueError(f"{cache}: TRANSCRIBE_X86_CONSERVATIVE must be ON")
    for key in SIMD:
        if values.get(key) not in (None, "OFF"):
            raise ValueError(f"{cache}: {key} enables a non-baseline x86 instruction set")


def main() -> int:
    caches = sorted(Path(sys.argv[1]).glob("transcribe-cpp-sys-*/out/build/CMakeCache.txt"))
    if not caches:
        print("No compiled transcribe-cpp-sys CMake cache found", file=sys.stderr)
        return 1
    try:
        for cache in caches:
            check(cache)
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1
    print(f"Portable ASR CPU configuration verified in {len(caches)} native build(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
