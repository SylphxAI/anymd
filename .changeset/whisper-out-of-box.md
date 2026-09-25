---
"@sylphx/anymd": minor
---

Transcripts work with less setup: anymd finds a whisper model in its cache, downloads a SHA-256-verified ggml model on first use when asked (`download_whisper_model: true`, `--download-whisper-model`, or `ANYMD_WHISPER_AUTO_DOWNLOAD=1`; size via `ANYMD_WHISPER_MODEL_SIZE`), accepts `ANYMD_WHISPER_BIN` and more whisper.cpp binary names, prints the exact install command for your OS when something is missing, and `anymd doctor` reports the model and cache path.
