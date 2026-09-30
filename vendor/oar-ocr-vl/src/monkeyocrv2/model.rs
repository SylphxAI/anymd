use super::config::{MonkeyOcrV2Config, MonkeyOcrV2ImageProcessorConfig};
use super::processing::{MonkeyOcrV2ImageInputs, preprocess_image};
use super::vision::MonkeyOcrV2VisionModel;
#[cfg(feature = "cuda")]
use crate::cuda_kernels::{ArgmaxFirstBf16, ArgmaxFirstF32};
use crate::doc_parser::{RecognitionBackend, RecognitionTask};
use crate::error::Error;
use crate::mineru_diffusion::text::{SdarKvCache, SdarModel};
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, Device, IndexOp, Tensor};
use image::RgbImage;
use std::path::Path;
use tokenizers::Tokenizer;

const MODEL_NAME: &str = "MonkeyOCRv2";

pub const DEFAULT_MAX_NEW_TOKENS: usize = 10_000;
pub const LAYOUT_MIN_PIXELS: u32 = 1_003_520;

/// Official MonkeyOCRv2-S/B-Parsing inference tasks and prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonkeyOcrV2Task {
    /// Detect document elements in reading order, returning normalized boxes and labels.
    Layout,
    /// Parse a full page in one pass, returning labels, boxes, and content.
    EndToEnd,
    /// Recognize ordinary text in an image or cropped document region.
    Text,
    /// Recognize a formula as LaTeX.
    Formula,
    /// Recognize a table in OTSL format.
    Table,
}

impl MonkeyOcrV2Task {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Layout => {
                "Please output the categories and coordinates of the document elements in reading order."
            }
            Self::EndToEnd => {
                "List the document elements in reading order, including their categories, coordinates, and the content of each element."
            }
            Self::Text => "Please output the text content from the image.",
            Self::Formula => {
                "Please write out the expression of the formula in the image using LaTeX format."
            }
            Self::Table => {
                "Please extract the table from the image and represent it in OTSL format."
            }
        }
    }
}

/// Candle-native MonkeyOCRv2-S/B-Parsing model (Monkey ViT-S/B + Qwen3-0.6B).
pub struct MonkeyOcrV2 {
    device: Device,
    dtype: DType,
    cfg: MonkeyOcrV2Config,
    image_cfg: MonkeyOcrV2ImageProcessorConfig,
    tokenizer: Tokenizer,
    vision: MonkeyOcrV2VisionModel,
    text: SdarModel,
    stop_token_ids: Vec<u32>,
    image_token_id: u32,
}

impl MonkeyOcrV2 {
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = MonkeyOcrV2Config::from_path(model_dir.join("config.json"))?;
        let image_cfg =
            MonkeyOcrV2ImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;
        image_cfg.validate_vision(&cfg.vision_config)?;
        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("MonkeyOCRv2 failed to load tokenizer.json: {e}"),
            })?;
        require_token(&tokenizer, "<|image_pad|>", Some(cfg.image_token_id))?;
        require_token(&tokenizer, "<|vision_start|>", None)?;
        require_token(&tokenizer, "<|vision_end|>", None)?;
        require_token(&tokenizer, "<|im_start|>", None)?;
        let im_end = require_token(&tokenizer, "<|im_end|>", None)?;
        let end_of_text = require_token(&tokenizer, "<|endoftext|>", None)?;

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, MODEL_NAME)?;
        // SAFETY: the mapped checkpoint files must not be modified while this model is alive.
        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load safetensors", e))?
        };
        let vision = MonkeyOcrV2VisionModel::load(&cfg.vision_config, vb.pp("vision_tower"))?;
        let text = SdarModel::load(&cfg.text_config, vb, dtype)?;
        let mut stop_token_ids = vec![im_end, end_of_text];
        if let Some(eos) = cfg.text_config.eos_token_id {
            stop_token_ids.push(eos);
        }
        if let Some(pad) = cfg.text_config.pad_token_id {
            stop_token_ids.push(pad);
        }
        stop_token_ids.sort_unstable();
        stop_token_ids.dedup();
        let image_token_id = cfg.image_token_id;
        Ok(Self {
            device,
            dtype,
            cfg,
            image_cfg,
            tokenizer,
            vision,
            text,
            stop_token_ids,
            image_token_id,
        })
    }

    /// Run official task prompts for one or more images.
    pub fn generate(
        &self,
        images: &[RgbImage],
        tasks: &[MonkeyOcrV2Task],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        Ok(self
            .generate_tokens(images, tasks, max_new_tokens)?
            .into_iter()
            .map(|result| result.and_then(|tokens| self.decode_tokens(&tokens)))
            .collect())
    }

    /// Run the model with caller-provided instructions.
    pub fn generate_with_prompts(
        &self,
        images: &[RgbImage],
        prompts: &[impl AsRef<str>],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        if images.len() != prompts.len() {
            return Err(length_mismatch(images.len(), prompts.len()));
        }
        Ok(images
            .iter()
            .zip(prompts)
            .map(|(image, prompt)| {
                self.generate_one(image, prompt.as_ref(), max_new_tokens, None)
                    .and_then(|tokens| self.decode_tokens(&tokens))
            })
            .collect())
    }

    /// Generate raw token ids with official task prompts.
    pub fn generate_tokens(
        &self,
        images: &[RgbImage],
        tasks: &[MonkeyOcrV2Task],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<Vec<u32>> {
        if images.len() != tasks.len() {
            return Err(length_mismatch(images.len(), tasks.len()));
        }
        Ok(images
            .iter()
            .zip(tasks)
            .map(|(image, &task)| {
                let min_pixels = (task == MonkeyOcrV2Task::Layout).then_some(LAYOUT_MIN_PIXELS);
                self.generate_one(image, task.prompt(), max_new_tokens, min_pixels)
            })
            .collect())
    }

    fn generate_one(
        &self,
        image: &RgbImage,
        instruction: &str,
        max_new_tokens: usize,
        min_pixels_override: Option<u32>,
    ) -> Result<Vec<u32>, Error> {
        if max_new_tokens == 0 {
            return Ok(Vec::new());
        }
        let mut image_cfg = self.image_cfg.clone();
        if let Some(min_pixels) = min_pixels_override {
            image_cfg.min_pixels = image_cfg.min_pixels.max(min_pixels);
            image_cfg.min_pixels = image_cfg.min_pixels.min(image_cfg.max_pixels);
        }
        let image_inputs = preprocess_image(image, &image_cfg, &self.device, self.dtype)?;
        let prompt = build_prompt(instruction, image_inputs.num_image_tokens);
        let encoding = self
            .tokenizer
            .encode(prompt, false)
            .map_err(|e| Error::InvalidInput {
                message: format!("MonkeyOCRv2 tokenizer encode failed: {e}"),
            })?;
        let input_ids = encoding.get_ids();
        if input_ids.is_empty() {
            return Err(Error::InvalidInput {
                message: "MonkeyOCRv2 prompt tokenization produced no tokens".to_string(),
            });
        }
        validate_generation_length(
            input_ids.len(),
            max_new_tokens,
            self.cfg.text_config.max_position_embeddings,
        )?;
        let inputs_embeds = self.prepare_inputs(input_ids, &image_inputs)?;
        let positions: Vec<i64> = (0..input_ids.len() as i64).collect();
        let mut cache = SdarKvCache::new(self.text.num_layers());
        let hidden = self
            .text
            .forward_causal(&inputs_embeds, &positions, &mut cache, true)?;
        let prompt_len = input_ids.len();
        let mut logits = self
            .text
            .lm_logits(
                &hidden
                    .i((.., prompt_len - 1..prompt_len, ..))
                    .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select prompt hidden", e))?,
            )?
            .i((0, 0, ..))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select prompt logits", e))?;

        let mut generated = Vec::new();
        generated
            .try_reserve_exact(max_new_tokens)
            .map_err(|e| Error::InvalidInput {
                message: format!(
                    "MonkeyOCRv2 cannot reserve output for {max_new_tokens} tokens: {e}"
                ),
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
            let token_ids = Tensor::new(&[[token]], &self.device)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create decode token", e))?;
            let token_embed = self.text.embed(&token_ids)?;
            let hidden = self.text.forward_causal(
                &token_embed,
                &[prompt_len as i64 + step as i64],
                &mut cache,
                true,
            )?;
            logits = self
                .text
                .lm_logits(&hidden)?
                .i((0, 0, ..))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select decode logits", e))?;
        }
        Ok(generated)
    }

    fn prepare_inputs(
        &self,
        input_ids: &[u32],
        image_inputs: &MonkeyOcrV2ImageInputs,
    ) -> Result<Tensor, Error> {
        let seq_len = input_ids.len();
        let token_ids = Tensor::new(input_ids, &self.device)
            .and_then(|tensor| tensor.unsqueeze(0))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create prompt token ids", e))?;
        let token_embeds = self.text.embed(&token_ids)?;
        let image_embeds = self
            .vision
            .forward(&image_inputs.pixel_values, image_inputs.grid_thw)?
            .to_dtype(self.dtype)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast image embeddings", e))?;
        let positions: Vec<usize> = input_ids
            .iter()
            .enumerate()
            .filter_map(|(index, &token)| (token == self.image_token_id).then_some(index))
            .collect();
        let image_len = image_embeds
            .dim(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read image embedding count", e))?;
        if positions.len() != image_len || positions.is_empty() {
            return Err(Error::InvalidInput {
                message: format!(
                    "MonkeyOCRv2 image placeholder count {} != image embedding count {image_len}",
                    positions.len()
                ),
            });
        }
        let start = positions[0];
        if positions
            .iter()
            .enumerate()
            .any(|(offset, &position)| position != start + offset)
        {
            return Err(Error::InvalidInput {
                message: "MonkeyOCRv2 image placeholders must be contiguous".to_string(),
            });
        }
        let end = start + image_len;
        let prefix = token_embeds
            .narrow(1, 0, start)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select embedding prefix", e))?;
        let image_embeds = image_embeds
            .unsqueeze(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "batch image embeddings", e))?;
        let suffix = token_embeds
            .narrow(1, end, seq_len - end)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select embedding suffix", e))?;
        Tensor::cat(&[&prefix, &image_embeds, &suffix], 1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "merge multimodal embeddings", e))
    }

    pub fn decode_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        self.tokenizer
            .decode(tokens, true)
            .map(|text| text.trim().to_string())
            .map_err(|e| Error::InvalidInput {
                message: format!("MonkeyOCRv2 tokenizer decode failed: {e}"),
            })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    pub fn config(&self) -> &MonkeyOcrV2Config {
        &self.cfg
    }

    pub fn image_processor_config(&self) -> &MonkeyOcrV2ImageProcessorConfig {
        &self.image_cfg
    }
}

impl RecognitionBackend for MonkeyOcrV2 {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        let monkey_task = match task {
            RecognitionTask::Ocr | RecognitionTask::Chart => MonkeyOcrV2Task::Text,
            RecognitionTask::Table => MonkeyOcrV2Task::Table,
            RecognitionTask::Formula => MonkeyOcrV2Task::Formula,
        };
        self.generate(&[image], &[monkey_task], max_tokens)?
            .pop()
            .ok_or_else(|| Error::InvalidInput {
                message: "MonkeyOCRv2: no recognition result returned".to_string(),
            })?
    }

    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        let monkey_tasks: Vec<MonkeyOcrV2Task> = tasks
            .iter()
            .map(|task| match task {
                RecognitionTask::Ocr | RecognitionTask::Chart => MonkeyOcrV2Task::Text,
                RecognitionTask::Table => MonkeyOcrV2Task::Table,
                RecognitionTask::Formula => MonkeyOcrV2Task::Formula,
            })
            .collect();
        self.generate(&images, &monkey_tasks, max_tokens)
    }

    fn needs_table_postprocess(&self) -> bool {
        true
    }

    fn needs_repetition_truncation(&self) -> bool {
        true
    }
}

fn build_prompt(instruction: &str, num_image_tokens: usize) -> String {
    let mut prompt = String::with_capacity(instruction.len() + num_image_tokens * 13 + 128);
    prompt.push_str("<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n");
    prompt.push_str("<|im_start|>user\n<|vision_start|>");
    for _ in 0..num_image_tokens {
        prompt.push_str("<|image_pad|>");
    }
    prompt.push_str("<|vision_end|>");
    prompt.push_str(instruction);
    prompt.push_str("<|im_end|>\n<|im_start|>assistant\n");
    prompt
}

fn require_token(tokenizer: &Tokenizer, token: &str, expected: Option<u32>) -> Result<u32, Error> {
    let id = tokenizer.token_to_id(token).ok_or_else(|| Error::Config {
        message: format!("MonkeyOCRv2 tokenizer is missing {token:?}"),
    })?;
    if let Some(expected) = expected
        && id != expected
    {
        return Err(Error::Config {
            message: format!(
                "MonkeyOCRv2 token {token:?} id mismatch: tokenizer {id} != config {expected}"
            ),
        });
    }
    Ok(id)
}

fn length_mismatch(images: usize, tasks: usize) -> Error {
    Error::InvalidInput {
        message: format!("MonkeyOCRv2 image count {images} != task/prompt count {tasks}"),
    }
}

fn validate_generation_length(
    prompt_len: usize,
    max_new_tokens: usize,
    context_limit: usize,
) -> Result<(), Error> {
    let requested = prompt_len
        .checked_add(max_new_tokens)
        .ok_or_else(|| Error::InvalidInput {
            message: "MonkeyOCRv2 requested sequence length overflows usize".to_string(),
        })?;
    if requested > context_limit {
        return Err(Error::InvalidInput {
            message: format!(
                "MonkeyOCRv2 prompt ({prompt_len}) plus max_new_tokens ({max_new_tokens}) exceeds context limit {context_limit}"
            ),
        });
    }
    Ok(())
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
                "MonkeyOCRv2 greedy argmax",
                e,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_matches_official_template() {
        assert_eq!(
            build_prompt("Question", 2),
            "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n<|vision_start|><|image_pad|><|image_pad|><|vision_end|>Question<|im_end|>\n<|im_start|>assistant\n"
        );
    }

    #[test]
    fn tasks_match_reference_prompts() {
        assert_eq!(
            MonkeyOcrV2Task::Table.prompt(),
            "Please extract the table from the image and represent it in OTSL format."
        );
        assert!(MonkeyOcrV2Task::EndToEnd.prompt().contains("reading order"));
    }

    #[test]
    fn greedy_argmax_prefers_first_tie() {
        let logits = Tensor::from_vec(vec![1f32, 3., 3., 2.], 4, &Device::Cpu).unwrap();
        assert_eq!(select_greedy_token(&logits).unwrap(), 1);
    }
}
