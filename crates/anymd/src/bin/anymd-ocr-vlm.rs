//! The VLM OCR companion: the in-process doc-VLM engine in its own executable,
//! so the default `anymd` binary stays small. `anymd setup ocr` installs it
//! next to the model weights and `anymd` runs it as the `__ocr-vlm-worker`
//! (page image path and token cap in argv, one JSON evidence document on
//! stdout), under the same supervision as before.

fn main() -> anyhow::Result<()> {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("version") {
        println!("anymd-ocr-vlm {}", anymd::SERVER_VERSION);
        return Ok(());
    }
    if arguments.first().map(String::as_str) == Some("__ocr-vlm-worker") {
        arguments.remove(0);
    }
    anymd::ocr_vlm::worker(&arguments).map_err(anyhow::Error::msg)
}
