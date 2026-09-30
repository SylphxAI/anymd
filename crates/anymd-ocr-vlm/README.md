# anymd-ocr-vlm

Local layout-first document OCR using PaddleOCR-VL-1.6 and PP-DocLayoutV3 through Candle (`oar-ocr-vl` 0.9.2). Both models are Apache-2.0. Enable `candle` for inference; the anymd binary enables it by default. macOS builds include Metal. Builds without the feature keep the result types and reading-order helpers.

Model installation belongs to `anymd setup ocr`, not this crate. No document is uploaded. The caller runs inference in a bounded subprocess so a page timeout actually stops compute and releases model memory. The decoder has a per-region token cap; repeated output is trimmed after recognition. Decode-time repetition cancellation is not implemented yet.

Regions carry top-left pixel boxes, layout confidence (not text confidence), reading order, and text. The binary adapter converts table HTML to Markdown and wraps formulas as display LaTeX before exposing OCR evidence.
