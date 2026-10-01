use super::config::{OvisOcr2Config, OvisOcr2ImageProcessorConfig};
use super::processing::{
    OvisOcr2ImageInputs, preprocess_image, validate_processor_vision_compatibility,
};
use super::text::OvisOcr2TextModel;
use super::vision::OvisOcr2VisionModel;
#[cfg(feature = "cuda")]
use crate::cuda_kernels::{ArgmaxFirstBf16, ArgmaxFirstF32};
use crate::doc_parser::{RecognitionBackend, RecognitionTask};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use image::RgbImage;
use std::path::Path;
use tokenizers::Tokenizer;

const MODEL_NAME: &str = "OvisOCR2";

/// Official OvisOCR2 full-page document parsing instruction.
pub const DEFAULT_PROMPT: &str = "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown. Preserve the original text without translation or paraphrasing.";

/// Upstream generation limit used by the official OvisOCR2 example.
pub const DEFAULT_MAX_NEW_TOKENS: usize = 16_384;

/// End-to-end OvisOCR2 page parser backed by Qwen3.5-0.8B.
pub struct OvisOcr2 {
    device: Device,
    dtype: DType,
    cfg: OvisOcr2Config,
    image_cfg: OvisOcr2ImageProcessorConfig,
    tokenizer: Tokenizer,
    text: OvisOcr2TextModel,
    vision: OvisOcr2VisionModel,
    lm_head: Linear,
    stop_token_ids: Vec<u32>,
    image_token_id: u32,
}

struct TextCacheGuard<'a>(&'a OvisOcr2TextModel);

impl Drop for TextCacheGuard<'_> {
    fn drop(&mut self) {
        self.0.clear_cache();
    }
}

impl OvisOcr2 {
    /// Load an OvisOCR2 Hugging Face model directory.
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = OvisOcr2Config::from_path(model_dir.join("config.json"))?;
        let image_cfg =
            OvisOcr2ImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;
        validate_processor_vision_compatibility(&image_cfg, &cfg.vision_config)?;
        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("failed to load OvisOCR2 tokenizer.json: {e}"),
            })?;
        require_token_id(&tokenizer, "<|image_pad|>", Some(cfg.image_token_id))?;
        require_token_id(
            &tokenizer,
            "<|vision_start|>",
            Some(cfg.vision_start_token_id),
        )?;
        require_token_id(&tokenizer, "<|vision_end|>", Some(cfg.vision_end_token_id))?;
        require_token_id(&tokenizer, "<|im_start|>", None)?;
        let tokenizer_eos = require_token_id(&tokenizer, "<|im_end|>", None)?;

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, MODEL_NAME)?;
        // SAFETY: The model files must remain unchanged while their mmap is in use.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load safetensors", e))?
        };
        let text = OvisOcr2TextModel::load(&cfg.text_config, vb.pp("model").pp("language_model"))?;
        let vision = OvisOcr2VisionModel::load(&cfg.vision_config, vb.pp("model").pp("visual"))?;
        // OvisOCR2 ties the language-model output projection to token embeddings.
        let lm_head = Linear::new(text.token_embedding_weight(), None);

        // vLLM's OvisOCR2 path stops on both the model-config EOS
        // (`<|endoftext|>`) and the tokenizer EOS (`<|im_end|>`).
        let stop_token_ids = build_stop_token_ids(cfg.text_config.eos_token_id, tokenizer_eos);
        let image_token_id = cfg.image_token_id;
        Ok(Self {
            device,
            dtype,
            cfg,
            image_cfg,
            tokenizer,
            text,
            vision,
            lm_head,
            stop_token_ids,
            image_token_id,
        })
    }

    /// Generate model-native Markdown, retaining visual-region image tags.
    pub fn generate(
        &self,
        images: &[RgbImage],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        Ok(self
            .generate_tokens(images, max_new_tokens)?
            .into_iter()
            .map(|result| result.and_then(|tokens| self.decode_tokens(&tokens)))
            .collect())
    }

    /// Parse pages using the official post-processing, which removes visual
    /// region `<img ...>` blocks by default.
    pub fn parse(
        &self,
        images: &[RgbImage],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        self.parse_with_image_tags(images, max_new_tokens, false)
    }

    /// Parse pages and optionally retain the model's visual-region image tags.
    pub fn parse_with_image_tags(
        &self,
        images: &[RgbImage],
        max_new_tokens: usize,
        keep_image_tags: bool,
    ) -> crate::error::BatchResult<String> {
        Ok(self
            .generate_tokens(images, max_new_tokens)?
            .into_iter()
            .map(|result| {
                result.and_then(|tokens| {
                    let text = self.decode_tokens_raw(&tokens)?;
                    Ok(postprocess_text(text, keep_image_tags))
                })
            })
            .collect())
    }

    /// Generate raw token ids for each input page.
    pub fn generate_tokens(
        &self,
        images: &[RgbImage],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<Vec<u32>> {
        Ok(images
            .iter()
            .map(|image| self.generate_one(image, max_new_tokens))
            .collect())
    }

    fn generate_one(&self, image: &RgbImage, max_new_tokens: usize) -> Result<Vec<u32>, Error> {
        self.text.clear_cache();
        let _cache_guard = TextCacheGuard(&self.text);
        if max_new_tokens == 0 {
            return Ok(Vec::new());
        }
        if max_new_tokens > self.cfg.text_config.max_position_embeddings {
            return Err(Error::InvalidInput {
                message: format!(
                    "OvisOCR2 max_new_tokens {max_new_tokens} exceeds context limit {}",
                    self.cfg.text_config.max_position_embeddings
                ),
            });
        }
        let image_inputs = preprocess_image(
            image,
            &self.image_cfg,
            &self.cfg.vision_config,
            &self.device,
            self.dtype,
        )?;
        let prompt = build_prompt(image_inputs.num_image_tokens);
        let encoding = self
            .tokenizer
            .encode(prompt, false)
            .map_err(|e| Error::InvalidInput {
                message: format!("OvisOCR2: tokenizer encode failed: {e}"),
            })?;
        let input_ids = encoding.get_ids().to_vec();
        if input_ids.is_empty() {
            return Err(Error::InvalidInput {
                message: "OvisOCR2: prompt tokenization produced no tokens".to_string(),
            });
        }
        validate_generation_length(
            input_ids.len(),
            max_new_tokens,
            self.cfg.text_config.max_position_embeddings,
        )?;

        let inputs_embeds = self.prepare_inputs(&input_ids, &image_inputs)?;
        let (position_ids, rope_delta) = build_position_ids(
            &input_ids,
            image_inputs.grid_thw,
            self.cfg.vision_config.spatial_merge_size,
            self.image_token_id,
            &self.device,
        )?;
        let hidden = self.text.forward(&inputs_embeds, &position_ids)?;
        let prompt_len = input_ids.len();
        let last_hidden = hidden
            .i((0, prompt_len - 1, ..))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select prompt hidden", e))?;
        let mut logits = self.logits_from_hidden(&last_hidden)?;
        let mut generated = Vec::new();
        generated
            .try_reserve_exact(max_new_tokens)
            .map_err(|e| Error::InvalidInput {
                message: format!("OvisOCR2 cannot reserve output for {max_new_tokens} tokens: {e}"),
            })?;

        for step in 0..max_new_tokens {
            let token = select_greedy_token(&logits)?;
            if self.stop_token_ids.contains(&token) {
                break;
            }
            generated.push(token);
            if step + 1 == max_new_tokens {
                break;
            }

            let token_ids = Tensor::from_vec(vec![token], (1, 1), &self.device).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "OvisOCR2: create decode token",
                    e,
                )
            })?;
            let token_embed = self.text.embed(&token_ids)?;
            let position = prompt_len as i64 + step as i64 + rope_delta;
            let position_ids = text_position_ids(position, &self.device)?;
            let hidden = self.text.forward(&token_embed, &position_ids)?;
            logits =
                self.logits_from_hidden(&hidden.i((0, 0, ..)).map_err(|e| {
                    candle_to_ocr_inference(MODEL_NAME, "select decode hidden", e)
                })?)?;
        }
        Ok(generated)
    }

    fn prepare_inputs(
        &self,
        input_ids: &[u32],
        image_inputs: &OvisOcr2ImageInputs,
    ) -> Result<Tensor, Error> {
        let seq_len = input_ids.len();
        let token_ids = Tensor::from_vec(input_ids.to_vec(), (1, seq_len), &self.device)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create prompt token ids", e))?;
        let embeds = self.text.embed(&token_ids)?;
        let image_embeds = self
            .vision
            .forward(&image_inputs.pixel_values, image_inputs.grid_thw)?
            .to_dtype(self.dtype)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast image embeddings", e))?;

        let image_positions: Vec<usize> = input_ids
            .iter()
            .enumerate()
            .filter_map(|(index, &token)| (token == self.image_token_id).then_some(index))
            .collect();
        let image_len = image_embeds
            .dim(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "image embedding length", e))?;
        if image_positions.len() != image_len || image_positions.is_empty() {
            return Err(Error::InvalidInput {
                message: format!(
                    "OvisOCR2: image placeholder count ({}) != image embedding count ({image_len})",
                    image_positions.len()
                ),
            });
        }
        let start = image_positions[0];
        if image_positions
            .iter()
            .enumerate()
            .any(|(offset, &position)| position != start + offset)
        {
            return Err(Error::InvalidInput {
                message: "OvisOCR2: image placeholder tokens must be contiguous".to_string(),
            });
        }
        let end = start + image_positions.len();
        let hidden_size = embeds
            .dim(2)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "embedding hidden size", e))?;
        let prefix = if start == 0 {
            Tensor::zeros((1, 0, hidden_size), embeds.dtype(), embeds.device())
        } else {
            embeds.narrow(1, 0, start)
        }
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "embedding prefix", e))?;
        let suffix = if end == seq_len {
            Tensor::zeros((1, 0, hidden_size), embeds.dtype(), embeds.device())
        } else {
            embeds.narrow(1, end, seq_len - end)
        }
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "embedding suffix", e))?;
        let image_embeds = image_embeds
            .unsqueeze(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "image embedding batch", e))?;
        Tensor::cat(&[&prefix, &image_embeds, &suffix], 1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "merge multimodal embeddings", e))
    }

    fn logits_from_hidden(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        self.lm_head
            .forward(
                &hidden
                    .unsqueeze(0)
                    .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "LM head input", e))?,
            )
            .and_then(|logits| logits.squeeze(0))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "language model head", e))
    }

    /// Decode generated token ids and apply the official truncated-repeat cleanup.
    pub fn decode_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        Ok(clean_truncated_repeats(&self.decode_tokens_raw(tokens)?))
    }

    /// Decode token ids without OvisOCR2 post-processing.
    pub fn decode_tokens_raw(&self, tokens: &[u32]) -> Result<String, Error> {
        self.tokenizer
            .decode(tokens, true)
            .map(|text| text.trim().to_string())
            .map_err(|e| Error::InvalidInput {
                message: format!("OvisOCR2: tokenizer decode failed: {e}"),
            })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    pub fn config(&self) -> &OvisOcr2Config {
        &self.cfg
    }

    pub fn image_processor_config(&self) -> &OvisOcr2ImageProcessorConfig {
        &self.image_cfg
    }
}

impl RecognitionBackend for OvisOcr2 {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        let tokens = self
            .generate_tokens(&[image], max_tokens)?
            .pop()
            .ok_or_else(|| Error::InvalidInput {
                message: "OvisOCR2: no recognition result returned".to_string(),
            })??;
        let text = self.decode_tokens_raw(&tokens)?;
        Ok(postprocess_recognition_text(text, task))
    }

    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        if images.len() != tasks.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "OvisOCR2: images count ({}) != tasks count ({})",
                    images.len(),
                    tasks.len()
                ),
            });
        }
        Ok(self
            .generate_tokens(&images, max_tokens)?
            .into_iter()
            .zip(tasks.iter().copied())
            .map(|(result, task)| {
                result.and_then(|tokens| {
                    let text = self.decode_tokens_raw(&tokens)?;
                    Ok(postprocess_recognition_text(text, task))
                })
            })
            .collect())
    }
}

fn require_token_id(
    tokenizer: &Tokenizer,
    token: &str,
    expected: Option<u32>,
) -> Result<u32, Error> {
    let token_id = tokenizer.token_to_id(token).ok_or_else(|| Error::Config {
        message: format!("OvisOCR2 tokenizer is missing required token {token:?}"),
    })?;
    if let Some(expected) = expected
        && token_id != expected
    {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 token {token:?} id mismatch: tokenizer {token_id} != config {expected}"
            ),
        });
    }
    Ok(token_id)
}

fn build_stop_token_ids(config_eos: u32, tokenizer_eos: u32) -> Vec<u32> {
    let mut token_ids = vec![config_eos, tokenizer_eos];
    token_ids.sort_unstable();
    token_ids.dedup();
    token_ids
}

fn validate_generation_length(
    prompt_len: usize,
    max_new_tokens: usize,
    context_limit: usize,
) -> Result<(), Error> {
    let requested = prompt_len
        .checked_add(max_new_tokens)
        .ok_or_else(|| Error::InvalidInput {
            message: "OvisOCR2 requested sequence length overflows usize".to_string(),
        })?;
    if requested > context_limit {
        return Err(Error::InvalidInput {
            message: format!(
                "OvisOCR2 prompt ({prompt_len}) plus max_new_tokens ({max_new_tokens}) exceeds context limit {context_limit}"
            ),
        });
    }
    Ok(())
}

fn build_prompt(num_image_tokens: usize) -> String {
    let mut prompt = String::with_capacity(DEFAULT_PROMPT.len() + num_image_tokens * 13 + 128);
    prompt.push_str("<|im_start|>user\n<|vision_start|>");
    for _ in 0..num_image_tokens {
        prompt.push_str("<|image_pad|>");
    }
    prompt.push_str("<|vision_end|>");
    prompt.push_str(DEFAULT_PROMPT);
    prompt.push_str("<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n");
    prompt
}

fn build_position_ids(
    input_ids: &[u32],
    grid_thw: (usize, usize, usize),
    spatial_merge_size: usize,
    image_token_id: u32,
    device: &Device,
) -> Result<(Tensor, i64), Error> {
    let image_start = input_ids
        .iter()
        .position(|&token| token == image_token_id)
        .ok_or_else(|| Error::InvalidInput {
            message: "OvisOCR2: image token missing from prompt".to_string(),
        })?;
    let image_len = input_ids
        .iter()
        .skip(image_start)
        .take_while(|&&token| token == image_token_id)
        .count();
    if input_ids[image_start + image_len..].contains(&image_token_id) {
        return Err(Error::InvalidInput {
            message: "OvisOCR2: non-contiguous image token span".to_string(),
        });
    }
    let (grid_t, grid_h, grid_w) = grid_thw;
    if spatial_merge_size == 0
        || !grid_h.is_multiple_of(spatial_merge_size)
        || !grid_w.is_multiple_of(spatial_merge_size)
    {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2: invalid image grid {grid_thw:?} for merge size {spatial_merge_size}"
            ),
        });
    }
    let llm_h = grid_h / spatial_merge_size;
    let llm_w = grid_w / spatial_merge_size;
    if image_len != grid_t * llm_h * llm_w {
        return Err(Error::InvalidInput {
            message: format!(
                "OvisOCR2: image token count {image_len} != merged grid token count {}",
                grid_t * llm_h * llm_w
            ),
        });
    }

    let seq_len = input_ids.len();
    let mut axes = [
        Vec::with_capacity(seq_len),
        Vec::with_capacity(seq_len),
        Vec::with_capacity(seq_len),
    ];
    for position in 0..image_start as i64 {
        for axis in &mut axes {
            axis.push(position);
        }
    }
    let vision_start = image_start as i64;
    for temporal in 0..grid_t {
        for row in 0..llm_h {
            for col in 0..llm_w {
                axes[0].push(vision_start + temporal as i64);
                axes[1].push(vision_start + row as i64);
                axes[2].push(vision_start + col as i64);
            }
        }
    }
    let text_start = vision_start + llm_h.max(llm_w) as i64;
    for (offset, _) in (image_start + image_len..seq_len).enumerate() {
        let current = text_start + offset as i64;
        for axis in &mut axes {
            axis.push(current);
        }
    }
    let max_position = axes
        .iter()
        .flat_map(|axis| axis.iter())
        .copied()
        .max()
        .unwrap_or(0);
    let rope_delta = max_position + 1 - seq_len as i64;
    let data: Vec<i64> = axes.into_iter().flatten().collect();
    let tensor = Tensor::from_vec(data, (3, 1, seq_len), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "OvisOCR2: create multimodal position ids",
            e,
        )
    })?;
    Ok((tensor, rope_delta))
}

fn text_position_ids(position: i64, device: &Device) -> Result<Tensor, Error> {
    Tensor::from_vec(vec![position; 3], (3, 1, 1), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "OvisOCR2: create decode position ids",
            e,
        )
    })
}

fn select_greedy_token(logits: &Tensor) -> Result<u32, Error> {
    #[cfg(feature = "cuda")]
    if logits.device().is_cuda() && matches!(logits.dtype(), DType::BF16 | DType::F32) {
        let vocab_size = logits.elem_count();
        let logits = logits
            .reshape((1, vocab_size))
            .and_then(|logits| logits.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape GPU logits", e))?;
        let tokens = match logits.dtype() {
            DType::BF16 => logits.apply_op1_no_bwd(&ArgmaxFirstBf16),
            DType::F32 => logits.apply_op1_no_bwd(&ArgmaxFirstF32),
            _ => unreachable!("dtype checked above"),
        }
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "stable GPU argmax", e))?;
        return tokens
            .i(0)
            .and_then(|token| token.to_scalar::<u32>())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "copy selected token", e));
    }

    logits
        .argmax(candle_core::D::Minus1)
        .and_then(|token| token.to_scalar::<u32>())
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "OvisOCR2: greedy argmax",
                e,
            )
        })
}

/// Remove model-emitted visual-region image-tag blocks, matching upstream.
pub fn filter_visual_image_tags(text: &str) -> String {
    text.split("\n\n")
        .filter(|block| !block.trim().starts_with("<img src=\"images/bbox_"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn postprocess_text(text: String, keep_image_tags: bool) -> String {
    let text = if keep_image_tags {
        text
    } else {
        filter_visual_image_tags(&text)
    };
    clean_truncated_repeats(&text)
}

fn postprocess_recognition_text(text: String, task: RecognitionTask) -> String {
    postprocess_text(text, task == RecognitionTask::Chart)
}

/// Clean a truncated repetitive tail using the official OvisOCR2 heuristic.
pub fn clean_truncated_repeats(text: &str) -> String {
    const MIN_TEXT_LEN: usize = 8_000;
    const MAX_PERIOD: usize = 200;
    const MIN_REPEAT_CHARS: usize = 100;
    const MIN_REPEAT_TIMES: usize = 5;

    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n < MIN_TEXT_LEN {
        return text.to_string();
    }
    for unit_len in 1..=MAX_PERIOD.min(n - 1) {
        if chars[n - 1] != chars[n - 1 - unit_len] {
            continue;
        }
        let mut match_len = 1usize;
        let mut index = n - 2;
        while index >= unit_len && chars[index] == chars[index - unit_len] {
            match_len += 1;
            index -= 1;
        }
        let total_len = match_len + unit_len;
        let repeat_times = total_len / unit_len;
        let tail_len = total_len % unit_len;
        if repeat_times >= MIN_REPEAT_TIMES && total_len >= MIN_REPEAT_CHARS {
            let prefix_end = n - total_len + unit_len;
            let mut output: String = chars[..prefix_end].iter().collect();
            output.extend(chars[n - tail_len..].iter());
            return output;
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_tokens_include_model_and_tokenizer_eos() {
        assert_eq!(build_stop_token_ids(248_044, 248_046), [248_044, 248_046]);
        assert_eq!(build_stop_token_ids(248_044, 248_044), [248_044]);
    }

    #[test]
    fn greedy_argmax_prefers_the_first_tied_token() {
        let logits = Tensor::from_vec(vec![1f32, 3., 3., 2.], 4, &Device::Cpu).unwrap();
        assert_eq!(select_greedy_token(&logits).unwrap(), 1);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_greedy_argmax_prefers_the_first_tied_token() -> candle_core::Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        for dtype in [DType::F32, DType::BF16] {
            let logits = Tensor::from_vec(vec![1f32, 3., 3., 2.], 4, &device)?.to_dtype(dtype)?;
            assert_eq!(select_greedy_token(&logits).unwrap(), 1);
        }
        Ok(())
    }

    #[test]
    fn generation_length_is_checked_without_overflow() {
        validate_generation_length(775, 16_384, 262_144).unwrap();
        assert!(validate_generation_length(775, 262_000, 262_144).is_err());
        assert!(validate_generation_length(1, usize::MAX, usize::MAX).is_err());
    }

    #[test]
    fn official_prompt_has_exact_no_think_framing() {
        let prompt = build_prompt(2);
        assert!(prompt.starts_with(
            "<|im_start|>user\n<|vision_start|><|image_pad|><|image_pad|><|vision_end|>\nExtract"
        ));
        assert!(prompt.ends_with("<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"));
    }

    #[test]
    fn layout_golden_position_ids_match_upstream() {
        let mut ids = vec![1u32; 775];
        ids[4..652].fill(248_056);
        let (positions, delta) =
            build_position_ids(&ids, (1, 54, 48), 2, 248_056, &Device::Cpu).unwrap();
        assert_eq!(delta, -621);
        let positions = positions.to_vec3::<i64>().unwrap();
        assert_eq!(positions[0][0][4], 4);
        assert_eq!(positions[1][0][651], 30);
        assert_eq!(positions[2][0][651], 27);
        assert_eq!(positions[0][0][652], 31);
        assert_eq!(positions[0][0][774], 153);
    }

    #[test]
    fn visual_image_tag_blocks_are_removed() {
        let text = "before\n\n<img src=\"images/bbox_1_2_3_4.jpg\" />\n\nafter";
        assert_eq!(filter_visual_image_tags(text), "before\n\nafter");
    }

    #[test]
    fn chart_recognition_keeps_visual_image_tags() {
        let text = "<img src=\"images/bbox_1_2_3_4.jpg\" />";
        assert_eq!(
            postprocess_recognition_text(text.to_string(), RecognitionTask::Chart),
            text
        );
    }

    #[test]
    fn mixed_recognition_tasks_filter_only_non_charts() {
        let text = "<img src=\"images/bbox_1_2_3_4.jpg\" />";
        let tasks = [
            RecognitionTask::Ocr,
            RecognitionTask::Chart,
            RecognitionTask::Table,
            RecognitionTask::Formula,
        ];
        let outputs = tasks.map(|task| postprocess_recognition_text(text.to_string(), task));

        assert_eq!(outputs, ["", text, "", ""]);
    }

    #[test]
    fn short_text_is_not_repeat_cleaned() {
        let text = "abc".repeat(100);
        assert_eq!(clean_truncated_repeats(&text), text);
    }

    #[test]
    fn long_repetitive_tail_is_cleaned() {
        let prefix = "x".repeat(8_000);
        let text = format!("{prefix}{}", "abcdef".repeat(30));
        let cleaned = clean_truncated_repeats(&text);
        assert!(cleaned.len() < text.len());
        assert!(cleaned.starts_with(&prefix));
    }
}
