import sys
from kreuzberg import extract_file_sync, ExtractionConfig
try:
    cfg = ExtractionConfig(output_format="markdown", use_cache=False)
except Exception as e:
    sys.stderr.write(f"cfg fallback: {e}\n"); cfg = ExtractionConfig(use_cache=False)
r = extract_file_sync(sys.argv[1], config=cfg)
sys.stdout.write(r.content)
