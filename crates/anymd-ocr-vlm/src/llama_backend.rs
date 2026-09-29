//! Option A: llama.cpp (mtmd) for PaddleOCR-VL-1.6 GGUF, ONNX Runtime for layout.

use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::Path;
use std::time::Instant;

use anyhow::{anyhow, Result};
use image::{imageops, RgbImage};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::layout_ort::LayoutOrt;
use crate::{DocOcr, PageOptions, PageResult, Region};

pub struct LlamaBackendOcr {
    // Field order is drop order: contexts before the model, the model before the backend.
    mtmd: MtmdContext,
    model: LlamaModel,
    backend: LlamaBackend,
    layout: LayoutOrt,
    threads: i32,
    gpu: bool,
}

impl LlamaBackendOcr {
    pub fn load(gguf: &Path, mmproj: &Path, layout_onnx: &Path, threads: usize, gpu: bool) -> Result<Self> {
        let backend = LlamaBackend::init().map_err(|e| anyhow!("llama backend: {e}"))?;
        let mut model_params = LlamaModelParams::default();
        if gpu {
            model_params = model_params.with_n_gpu_layers(1_000_000);
        }
        let model = LlamaModel::load_from_file(&backend, gguf, &model_params).map_err(|e| anyhow!("gguf: {e}"))?;
        let params = MtmdContextParams {
            use_gpu: gpu,
            print_timings: false,
            n_threads: threads as i32,
            media_marker: CString::new(llama_cpp_2::mtmd::mtmd_default_marker())?,
            image_min_tokens: -1,
            image_max_tokens: -1,
        };
        let mmproj_str = mmproj.to_str().ok_or_else(|| anyhow!("mmproj path is not UTF-8"))?;
        let mtmd = MtmdContext::init_from_file(mmproj_str, &model, &params).map_err(|e| anyhow!("mmproj: {e}"))?;
        let layout = LayoutOrt::load(layout_onnx, threads)?;
        Ok(Self { mtmd, model, backend, layout, threads: threads as i32, gpu })
    }

    /// Greedy decode of one crop. Returns the text and whether it was cut short.
    fn recognise(&self, crop: &RgbImage, prompt: &str, max_new: usize, deadline: Instant) -> Result<(String, bool)> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(4096))
            .with_n_batch(1024)
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads);
        let mut ctx = self.model.new_context(&self.backend, ctx_params).map_err(|e| anyhow!("context: {e}"))?;
        let bitmap = MtmdBitmap::from_image_data(crop.width(), crop.height(), crop.as_raw())
            .map_err(|e| anyhow!("bitmap: {e}"))?;
        // Chat template of the official GGUF (chat_template.jinja): BOS, "User: ", image, task, "\nAssistant:\n".
        let text = format!(
            "<|begin_of_sentence|>User: {}{}\nAssistant:\n",
            llama_cpp_2::mtmd::mtmd_default_marker(),
            prompt
        );
        let chunks = self
            .mtmd
            .tokenize(MtmdInputText { text, add_special: false, parse_special: true }, &[&bitmap])
            .map_err(|e| anyhow!("tokenize: {e}"))?;
        let mut n_past = chunks
            .eval_chunks(&self.mtmd, &ctx, 0, 0, 512, true)
            .map_err(|e| anyhow!("eval: {e}"))?;

        let mut sampler = LlamaSampler::chain_simple([LlamaSampler::greedy()]);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut batch = LlamaBatch::new(1, 1);
        let mut out = String::new();
        let mut generated: Vec<LlamaToken> = Vec::new();
        let mut cut = false;
        for step in 0..max_new {
            let token = sampler.sample(&ctx, -1);
            sampler.accept(token);
            if self.model.is_eog_token(token) {
                break;
            }
            generated.push(token);
            // A piece that fails to decode is skipped, never fatal (PaddleOCR issue #18170).
            if let Ok(piece) = self.model.token_to_piece(token, &mut decoder, true, None) {
                out.push_str(&piece);
            }
            if step % 16 == 15 && (Instant::now() >= deadline || repeating(&generated)) {
                cut = true;
                break;
            }
            batch.clear();
            batch.add(token, n_past, &[0], true).map_err(|e| anyhow!("batch: {e}"))?;
            n_past += 1;
            ctx.decode(&mut batch).map_err(|e| anyhow!("decode: {e}"))?;
            if step + 1 == max_new {
                cut = true;
            }
        }
        Ok((out, cut))
    }
}

/// True when the tail of `tokens` is one short pattern repeated at least eight times.
fn repeating(tokens: &[LlamaToken]) -> bool {
    for period in 1..=32usize {
        let need = period * 8;
        if tokens.len() < need {
            break;
        }
        let tail = &tokens[tokens.len() - need..];
        if tail.chunks(period).all(|c| c == &tail[..period]) {
            return true;
        }
    }
    false
}

fn task_prompt(label: &str) -> Option<&'static str> {
    match label {
        "table" => Some("Table Recognition:"),
        "display_formula" => Some("Formula Recognition:"),
        // Non-text regions and running headers/footers are not recognised.
        "image" | "chart" | "seal" | "header_image" | "footer_image" | "header" | "footer" | "number"
        | "formula_number" | "inline_formula" => None,
        _ => Some("OCR:"),
    }
}

impl DocOcr for LlamaBackendOcr {
    fn name(&self) -> &'static str {
        "llama.cpp mtmd + ort"
    }

    fn recognize_page(&mut self, page: &RgbImage, options: &PageOptions) -> Result<PageResult> {
        let deadline = Instant::now() + options.page_timeout;
        let mut regions = self.layout.detect(page)?;
        let mut truncated = 0;
        let _ = self.gpu;
        for region in &mut regions {
            let Some(prompt) = task_prompt(&region.label) else { continue };
            let [x0, y0, x1, y1] = region.bbox;
            let (x, y) = (x0.floor() as u32, y0.floor() as u32);
            let (w, h) = ((x1.ceil() as u32).saturating_sub(x), (y1.ceil() as u32).saturating_sub(y));
            if w < 8 || h < 8 || Instant::now() >= deadline {
                if Instant::now() >= deadline {
                    truncated += 1;
                }
                continue;
            }
            let crop = imageops::crop_imm(page, x, y, w, h).to_image();
            match self.recognise(&crop, prompt, options.max_new_tokens, deadline) {
                Ok((text, cut)) => {
                    region.text = text;
                    truncated += u32::from(cut);
                }
                // One bad region must not fail the page.
                Err(e) => eprintln!("region {} failed: {e:#}", region.order),
            }
        }
        Ok(PageResult { regions, truncated })
    }
}
