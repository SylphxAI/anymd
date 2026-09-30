# anymd OAR OCR VL fork

Fork of `oar-ocr-vl` 0.9.2 ([upstream](https://github.com/greatv/oar-ocr)), licensed Apache-2.0. Source is from the crates.io release.

Local changes: stop repeated token runs during PaddleOCR-VL decoding, expose stop counts, and optionally quantize its Ernie decoder linear weights on CPU with Candle GGML q8_0/q4_0 matmul. Vision, embeddings and layout stay unquantized. The pinned safetensors download stays unchanged; quantization happens at model load. `ANYMD_OCR_QUANTIZATION=none|q8|q4` controls this experimental route; none is the default. GPU quantization is rejected.

This fork exists because the upstream API has no decode callback or quantized PaddleOCR-VL loader. No other model behaviour is intentionally changed.
