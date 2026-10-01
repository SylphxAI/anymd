//! PaddleOCR-VL (Vision-Language) model implementation.

use super::config::{PaddleOcrVlConfig, PaddleOcrVlImageProcessorConfig};
use super::ernie::Ernie4_5Model;
use super::processing;
use super::projector::Projector;
use super::quantized_linear::{self, Linear};
use super::vision::VisionModel;
use crate::attention::{
    combine_masks, create_causal_mask, create_generation_mask_if_needed, create_left_padding_mask,
};
#[cfg(feature = "cuda")]
use crate::decoder_graph::CudaGraphDrainGuard;
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::Module;
use image::{RgbImage, imageops::FilterType};
use std::path::Path;
use tokenizers::Tokenizer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaddleOcrVlTask {
    Ocr,
    Table,
    Chart,
    Formula,
    Spotting,
    Seal,
}

impl PaddleOcrVlTask {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Ocr => "OCR:",
            Self::Table => "Table Recognition:",
            Self::Chart => "Chart Recognition:",
            Self::Formula => "Formula Recognition:",
            Self::Spotting => "Spotting:",
            Self::Seal => "Seal Recognition:",
        }
    }

    pub fn postprocess(self, text: String) -> String {
        let trimmed = text.trim();
        match self {
            Self::Formula => processing::strip_math_wrappers(trimmed).to_string(),
            Self::Table => processing::postprocess_table_output(trimmed),
            Self::Ocr | Self::Chart | Self::Spotting | Self::Seal => trimmed.to_string(),
        }
    }

    fn needs_spotting_preprocess(self) -> bool {
        matches!(self, Self::Spotting)
    }
}

const SPOTTING_UPSCALE_THRESHOLD: u32 = 1500;
const SPOTTING_MAX_LONG_SIDE: u32 = 2048;

pub struct PaddleOcrVl {
    device: Device,
    dtype: DType,
    cfg: PaddleOcrVlConfig,
    image_cfg: PaddleOcrVlImageProcessorConfig,
    tokenizer: Tokenizer,
    llm: Ernie4_5Model,
    lm_head: Linear,
    generation_stops: std::cell::Cell<usize>,
    vision: VisionModel,
    projector: Projector,
    eos_token_id: u32,
    sep_token_id: Option<u32>,
    /// Cached tokenized image placeholder (single token)
    image_placeholder_token_id: u32,
    /// Assistant prefix derived from chat template (e.g., "Assistant: " vs "Assistant:\n")
    assistant_prefix: String,
    // Must stay the last field: the captured decoder graphs read the LM head
    // owned here, so this guard drops last and drains CUDA errors the head's
    // free may stash (see CudaGraphDrainGuard).
    #[cfg(feature = "cuda")]
    _drain_guard: CudaGraphDrainGuard,
}

impl PaddleOcrVl {
    /// Accumulated token-cap/repetition stops since the last page reset.
    pub fn generation_stops(&self) -> usize {
        self.generation_stops.get()
    }
    pub fn reset_generation_stops(&self) {
        self.generation_stops.set(0);
    }

    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = PaddleOcrVlConfig::from_path(model_dir.join("config.json"))?;
        let image_cfg =
            PaddleOcrVlImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;

        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("failed to load PaddleOCR-VL tokenizer.json: {e}"),
            })?;

        let eos_token_id = tokenizer.token_to_id("</s>").ok_or_else(|| Error::Config {
            message: "PaddleOCR-VL: tokenizer is missing </s> token".to_string(),
        })?;
        let sep_token_id = tokenizer.token_to_id("<|end_of_sentence|>");

        let assistant_prefix = if std::fs::read_to_string(model_dir.join("chat_template.jinja"))
            .map(|t| t.contains("Assistant:\\n"))
            .unwrap_or(false)
        {
            "Assistant:\n".to_string()
        } else {
            "Assistant: ".to_string()
        };

        // Pre-tokenize image placeholder to avoid repeated string allocation
        let image_placeholder_token_id = tokenizer
            .token_to_id("<|IMAGE_PLACEHOLDER|>")
            .ok_or_else(|| Error::Config {
                message: "PaddleOCR-VL: tokenizer is missing <|IMAGE_PLACEHOLDER|> token"
                    .to_string(),
            })?;

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, "PaddleOCR-VL")?;
        // SAFETY: from_mmaped_safetensors memory-maps the weight files directly;
        // the caller must ensure they are valid and not modified while in use.
        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load safetensors", e))?
        };

        let llm = Ernie4_5Model::load(&cfg, vb.pp("model"))?;
        let lm_head =
            quantized_linear::linear_no_bias(cfg.hidden_size, cfg.vocab_size, vb.pp("lm_head"))
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load lm_head", e))?;

        let vision = VisionModel::load(&cfg.vision_config, vb.pp("visual").pp("vision_model"))?;
        let projector = Projector::load(&cfg, &cfg.vision_config, vb.pp("mlp_AR"))?;

        #[cfg(feature = "cuda")]
        let _drain_guard = CudaGraphDrainGuard::new(&device);
        Ok(Self {
            device,
            dtype,
            cfg,
            image_cfg,
            tokenizer,
            llm,
            lm_head,
            generation_stops: std::cell::Cell::new(0),
            vision,
            projector,
            eos_token_id,
            sep_token_id,
            image_placeholder_token_id,
            assistant_prefix,
            #[cfg(feature = "cuda")]
            _drain_guard,
        })
    }

    /// Image-token count used to form padding-free document-parser batches.
    pub(crate) fn recognition_batch_key(&self, image: &RgbImage) -> Option<u64> {
        let patch = self.image_cfg.patch_size as u32;
        let merge = self.image_cfg.merge_size;
        let factor = patch.checked_mul(merge as u32)?;
        let (height, width) = if self.image_cfg.do_resize {
            processing::smart_resize(
                image.height(),
                image.width(),
                factor,
                self.image_cfg.min_pixels,
                self.image_cfg.max_pixels,
            )
            .ok()?
        } else {
            (image.height(), image.width())
        };
        let grid_h = height.checked_div(patch)? as usize;
        let grid_w = width.checked_div(patch)? as usize;
        grid_h
            .checked_mul(grid_w)?
            .checked_div(merge.checked_mul(merge)?)
            .map(|tokens| tokens as u64)
    }

    /// Generate OCR output for one or more images.
    ///
    /// Supports true GPU batching when multiple images are provided.
    ///
    /// # Arguments
    /// * `images` - Input images
    /// * `tasks` - Task for each image (must match images length)
    /// * `max_new_tokens` - Maximum tokens to generate per image
    ///
    /// # Returns
    /// Vector of results, one per input image.
    pub fn generate(
        &self,
        images: &[RgbImage],
        tasks: &[PaddleOcrVlTask],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        Ok(self
            .generate_with_raw(images, tasks, max_new_tokens)?
            .into_iter()
            .map(|r| r.map(|(_, processed)| processed))
            .collect())
    }

    /// Generate with both raw and postprocessed output.
    pub fn generate_with_raw(
        &self,
        images: &[RgbImage],
        tasks: &[PaddleOcrVlTask],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<(String, String)> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        if images.len() != tasks.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "PaddleOCR-VL: images count ({}) != tasks count ({})",
                    images.len(),
                    tasks.len()
                ),
            });
        }

        let results = self.generate_tokens_internal(images, tasks, max_new_tokens)?;
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(i, tokens)| self.decode_generated_tokens(&tokens, tasks[i]))
            .collect())
    }

    /// Generate raw baseline tokens for oracle-draft / tokenizer round-trip
    /// experiments. Tokens are exactly the ids emitted by the decode loop,
    /// excluding EOS / separator stop tokens, before tokenizer decoding or
    /// task postprocessing.
    pub fn generate_tokens(
        &self,
        images: &[RgbImage],
        tasks: &[PaddleOcrVlTask],
        max_new_tokens: usize,
    ) -> Result<Vec<Vec<u32>>, Error> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        if images.len() != tasks.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "PaddleOCR-VL: images count ({}) != tasks count ({})",
                    images.len(),
                    tasks.len()
                ),
            });
        }
        self.generate_tokens_internal(images, tasks, max_new_tokens)
    }

    /// Internal generation implementation supporting batched inference.
    fn generate_tokens_internal(
        &self,
        images: &[RgbImage],
        tasks: &[PaddleOcrVlTask],
        max_new_tokens: usize,
    ) -> Result<Vec<Vec<u32>>, Error> {
        let batch_size = images.len();

        // 1. Preprocess all images
        let needs_spotting = tasks.iter().any(|t| t.needs_spotting_preprocess());
        let resized_images: Vec<RgbImage>;
        let images_for_preprocess = if needs_spotting {
            // The official spotting preprocessing hardcodes LANCZOS for the 2x
            // pre-upscale (README inference script), independent of the
            // processor's `resample` (which only governs the model-internal
            // smart_resize). Match it with Lanczos3 here.
            let resize_filter = FilterType::Lanczos3;
            let mut processed = Vec::with_capacity(images.len());
            for (img, task) in images.iter().zip(tasks.iter()) {
                if task.needs_spotting_preprocess()
                    && img.width() < SPOTTING_UPSCALE_THRESHOLD
                    && img.height() < SPOTTING_UPSCALE_THRESHOLD
                {
                    processed.push(image::imageops::resize(
                        img,
                        img.width().saturating_mul(2),
                        img.height().saturating_mul(2),
                        resize_filter,
                    ));
                } else {
                    processed.push(img.clone());
                }
            }
            resized_images = processed;
            resized_images.as_slice()
        } else {
            images
        };
        let max_pixels = if needs_spotting {
            let factor = (self.image_cfg.patch_size * self.image_cfg.merge_size) as u32;
            let spotting_max_pixels = SPOTTING_MAX_LONG_SIDE
                .saturating_mul(factor)
                .saturating_mul(factor);
            self.image_cfg.max_pixels.max(spotting_max_pixels)
        } else {
            self.image_cfg.max_pixels
        };
        let image_inputs = processing::preprocess_images_with_max_pixels(
            images_for_preprocess,
            &self.image_cfg,
            &self.device,
            self.dtype,
            max_pixels,
        )?;

        // 2. Build prompts for each sample
        let mut all_input_ids: Vec<Vec<u32>> = Vec::with_capacity(batch_size);
        let mut all_image_token_counts: Vec<usize> = Vec::with_capacity(batch_size);

        for (i, task) in tasks.iter().enumerate() {
            let (t, h, w) = image_inputs.image_grid_thw[i];
            let image_token_count =
                (t * h * w) / (self.image_cfg.merge_size * self.image_cfg.merge_size);
            all_image_token_counts.push(image_token_count);

            let prefix = "<|begin_of_sentence|>User: <|IMAGE_START|>";
            let suffix = format!("<|IMAGE_END|>{}\n{}", task.prompt(), self.assistant_prefix);

            let prefix_enc =
                self.tokenizer
                    .encode(prefix, false)
                    .map_err(|e| Error::InvalidInput {
                        message: format!("tokenizer encode failed: {e}"),
                    })?;
            let suffix_enc =
                self.tokenizer
                    .encode(suffix.as_str(), false)
                    .map_err(|e| Error::InvalidInput {
                        message: format!("tokenizer encode failed: {e}"),
                    })?;

            let mut input_ids =
                Vec::with_capacity(prefix_enc.len() + image_token_count + suffix_enc.len());
            input_ids.extend_from_slice(prefix_enc.get_ids());
            input_ids.extend(std::iter::repeat_n(
                self.image_placeholder_token_id,
                image_token_count,
            ));
            input_ids.extend_from_slice(suffix_enc.get_ids());
            all_input_ids.push(input_ids);
        }

        // 3. Compute vision features
        let vision_feats = self
            .vision
            .forward(&image_inputs.pixel_values, &image_inputs.image_grid_thw)?;
        let image_embeds_all = self
            .projector
            .forward(&vision_feats, &image_inputs.image_grid_thw)?;

        // 4. Build embeddings per sample
        let seq_lens: Vec<usize> = all_input_ids.iter().map(|ids| ids.len()).collect();
        let Some(&max_seq_len) = seq_lens.iter().max() else {
            return Err(Error::InvalidInput {
                message: "PaddleOCR-VL: empty batch is not supported".to_string(),
            });
        };

        let mut batch_embeds: Vec<Tensor> = Vec::with_capacity(batch_size);
        let mut rope_deltas: Vec<i64> = Vec::with_capacity(batch_size);
        let mut batch_position_ids: Vec<Tensor> = Vec::with_capacity(batch_size);
        let mut embed_offset = 0usize;

        for (i, input_ids) in all_input_ids.iter().enumerate() {
            let seq_len = input_ids.len();
            let pad_len = max_seq_len - seq_len;
            let image_token_count = all_image_token_counts[i];

            // Get image embeddings for this sample
            let image_embeds = image_embeds_all
                .narrow(0, embed_offset, image_token_count)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "narrow image embeds", e))?;
            embed_offset += image_token_count;

            // Embed tokens
            let input_ids_t = Tensor::new(input_ids.clone(), &self.device)
                .and_then(|t| t.reshape((1, seq_len)))
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create input_ids", e))?;
            let mut inputs_embeds = self.llm.embed(&input_ids_t)?;

            // Fuse image embeddings
            let first_img_pos = input_ids
                .iter()
                .position(|&id| id == self.cfg.image_token_id);
            if let Some(first_pos) = first_img_pos {
                let image_end = first_pos + image_token_count;
                let mut parts: Vec<Tensor> = Vec::with_capacity(3);

                if first_pos > 0 {
                    parts.push(inputs_embeds.narrow(1, 0, first_pos).map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "narrow prefix", e)
                    })?);
                }
                parts.push(
                    image_embeds
                        .unsqueeze(0)
                        .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "unsqueeze img", e))?,
                );
                if image_end < seq_len {
                    parts.push(
                        inputs_embeds
                            .narrow(1, image_end, seq_len - image_end)
                            .map_err(|e| {
                                candle_to_ocr_inference("PaddleOCR-VL", "narrow suffix", e)
                            })?,
                    );
                }

                let refs: Vec<&Tensor> = parts.iter().collect();
                inputs_embeds = Tensor::cat(&refs, 1)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "cat embeds", e))?;
            }

            // Left-pad if needed
            if pad_len > 0 {
                let pad = Tensor::zeros(
                    (1, pad_len, self.cfg.hidden_size),
                    inputs_embeds.dtype(),
                    &self.device,
                )
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create pad", e))?;
                inputs_embeds = Tensor::cat(&[&pad, &inputs_embeds], 1)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "cat pad", e))?;
            }
            batch_embeds.push(inputs_embeds);

            // Position IDs
            let (pos_ids, delta) = get_rope_index(
                &self.cfg,
                input_ids,
                &[image_inputs.image_grid_thw[i]],
                &self.device,
            )?;
            rope_deltas.push(delta);

            let pos_ids = if pad_len > 0 {
                let pad_pos = Tensor::zeros((3, 1, pad_len), DType::I64, &self.device)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create pad pos", e))?;
                Tensor::cat(&[&pad_pos, &pos_ids], 2)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "cat pad pos", e))?
            } else {
                pos_ids
            };
            batch_position_ids.push(pos_ids);
        }

        // 5. Stack batched tensors
        let batch_refs: Vec<&Tensor> = batch_embeds.iter().collect();
        let inputs_embeds = Tensor::cat(&batch_refs, 0)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "stack embeds", e))?;

        let pos_refs: Vec<&Tensor> = batch_position_ids.iter().collect();
        let position_ids = Tensor::cat(&pos_refs, 1)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "stack pos", e))?;

        // 6. Create attention mask
        let mask = if batch_size == 1 {
            None
        } else {
            let causal = create_causal_mask(max_seq_len, max_seq_len, self.dtype, &self.device)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create causal", e))?;
            let padding =
                create_left_padding_mask(&seq_lens, max_seq_len, self.dtype, &self.device)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create padding", e))?;
            Some(
                combine_masks(&causal, &padding)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "combine masks", e))?,
            )
        };

        // 7. Prefill
        self.llm.clear_kv_cache();
        if batch_size == 1 {
            self.llm
                .prepare_ar_cuda_graph(max_seq_len, max_new_tokens, &self.lm_head)?;
        } else {
            // A batch-shaped prefill reallocates the per-layer KV storage. Drop
            // a batch-1 graph before that happens because it owns raw pointers
            // into the previous fixed storage.
            self.llm.invalidate_ar_cuda_graph();
        }
        let hidden = self
            .llm
            .forward(&inputs_embeds, &position_ids, mask.as_ref())?;

        // 8. Get initial logits
        let last_hidden = hidden
            .i((.., max_seq_len - 1, ..))
            .and_then(|hidden| hidden.contiguous())
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "get last hidden", e))?;
        let batched_logits = self
            .lm_head
            .forward(&last_hidden)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "lm_head", e))?;
        let mut logits_list: Vec<Tensor> = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            logits_list.push(
                batched_logits
                    .i(i)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "select logits", e))?,
            );
        }

        // 9. Autoregressive decode
        let mut generated: Vec<Vec<u32>> = vec![Vec::new(); batch_size];
        let mut finished: Vec<bool> = vec![false; batch_size];
        let mut positions: Vec<i64> = seq_lens
            .iter()
            .zip(&rope_deltas)
            .map(|(&len, &d)| (len as i64) + d)
            .collect();

        // Left-padding lengths per row, and current KV-cache length (grows by one
        // each decode step). Used to mask out padding KV during generation so a
        // batch with unequal prompt lengths does not attend to padding positions.
        let pad_lens: Vec<usize> = seq_lens.iter().map(|&len| max_seq_len - len).collect();
        let mut kv_len = max_seq_len;

        for step in 0..max_new_tokens {
            if finished.iter().all(|&f| f) {
                break;
            }
            let mut next_tokens: Vec<u32> = Vec::with_capacity(batch_size);
            for (i, logits) in logits_list.iter().enumerate() {
                if finished[i] {
                    next_tokens.push(0);
                } else {
                    let tok = logits
                        .argmax(D::Minus1)
                        .and_then(|t| t.to_scalar::<u32>())
                        .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "argmax", e))?;

                    if tok == self.eos_token_id || self.sep_token_id.is_some_and(|id| id == tok) {
                        finished[i] = true;
                    } else {
                        generated[i].push(tok);
                        if let Some(keep) = repeated_suffix(&generated[i]) {
                            generated[i].truncate(keep);
                            finished[i] = true;
                            self.generation_stops.set(self.generation_stops.get() + 1);
                        }
                    }
                    next_tokens.push(tok);
                }
            }

            if finished.iter().all(|&f| f) {
                break;
            }
            if step + 1 == max_new_tokens {
                break;
            }

            // Batch forward
            let tokens = Tensor::new(next_tokens, &self.device)
                .and_then(|t| t.reshape((batch_size, 1)))
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create tokens", e))?;
            let embeds = self.llm.embed(&tokens)?;

            let pos_data = crate::attention::decode_position_buffer(&positions, 3);
            let pos = Tensor::new(pos_data, &self.device)
                .and_then(|t| t.reshape((3, batch_size, 1)))
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create pos", e))?;

            // Mask out left-padding positions in the KV cache for this step.
            kv_len += 1;
            let gen_mask = if batch_size == 1 {
                None
            } else {
                create_generation_mask_if_needed(&pad_lens, kv_len, self.dtype, &self.device)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "create gen mask", e))?
            };

            logits_list.clear();
            if batch_size == 1 {
                logits_list.push(self.llm.forward_decode_logits(
                    &embeds,
                    &pos,
                    gen_mask.as_ref(),
                    &self.lm_head,
                )?);
            } else {
                let hs = self.llm.forward(&embeds, &pos, gen_mask.as_ref())?;
                let batched_logits = self
                    .lm_head
                    .forward(&hs)
                    .and_then(|t| t.squeeze(1))
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "lm_head step", e))?;
                for i in 0..batch_size {
                    logits_list.push(batched_logits.i(i).map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "select step logits", e)
                    })?);
                }
            }

            for (i, p) in positions.iter_mut().enumerate() {
                if !finished[i] {
                    *p += 1;
                }
            }
        }

        for (tokens, done) in generated.iter().zip(&finished) {
            if !done && tokens.len() == max_new_tokens {
                self.generation_stops.set(self.generation_stops.get() + 1);
            }
        }
        Ok(generated)
    }

    pub fn decode_tokens(
        &self,
        tokens: &[u32],
        task: PaddleOcrVlTask,
    ) -> Result<(String, String), Error> {
        self.decode_generated_tokens(tokens, task)
    }

    /// Decode tokens **without** applying PaddleOCR-VL's task-specific
    /// post-process (OTSL-to-HTML for tables, `$$..$$` stripping for formulas).
    /// This is the raw pre-postprocess string the model actually emitted —
    /// use this when feeding PaddleOCR-VL output as a draft for another
    /// target VLM. DSV matches at token granularity, so any post-process on
    /// the source side will byte-mismatch the target's natural output.
    pub fn decode_tokens_raw(&self, tokens: &[u32]) -> Result<String, Error> {
        self.tokenizer
            .decode(tokens, true)
            .map_err(|e| Error::InvalidInput {
                message: format!("decode failed: {e}"),
            })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    fn decode_generated_tokens(
        &self,
        tokens: &[u32],
        task: PaddleOcrVlTask,
    ) -> Result<(String, String), Error> {
        let decoded = self.decode_tokens_raw(tokens)?;
        let processed = task.postprocess(decoded.clone());
        Ok((decoded, processed))
    }
}

fn get_rope_index(
    cfg: &PaddleOcrVlConfig,
    input_ids: &[u32],
    image_grid_thw: &[(usize, usize, usize)],
    device: &Device,
) -> Result<(Tensor, i64), Error> {
    let spatial_merge_size = cfg.vision_config.spatial_merge_size;
    if spatial_merge_size == 0 {
        return Err(Error::Config {
            message: "PaddleOCR-VL: vision_config.spatial_merge_size must be > 0".to_string(),
        });
    }

    let mut image_count = 0usize;
    for i in 0..input_ids.len().saturating_sub(1) {
        if input_ids[i] == cfg.vision_start_token_id && input_ids[i + 1] == cfg.image_token_id {
            image_count += 1;
        }
        if input_ids[i] == cfg.vision_start_token_id && input_ids[i + 1] == cfg.video_token_id {
            return Err(Error::InvalidInput {
                message: "PaddleOCR-VL: video inputs are not supported".to_string(),
            });
        }
    }
    if image_count != image_grid_thw.len() {
        return Err(Error::InvalidInput {
            message: format!(
                "PaddleOCR-VL: image count mismatch between prompt ({image_count}) and image_grid_thw ({})",
                image_grid_thw.len()
            ),
        });
    }

    let mut positions: Vec<[i64; 3]> = Vec::with_capacity(input_ids.len());
    let mut st = 0usize;
    let mut current_max: i64 = -1;

    for (image_idx, &(t, h, w)) in image_grid_thw.iter().enumerate() {
        let ed = input_ids[st..]
            .iter()
            .position(|&id| id == cfg.image_token_id)
            .map(|p| st + p)
            .ok_or_else(|| Error::InvalidInput {
                message: format!(
                    "PaddleOCR-VL: expected image token for image[{image_idx}] but none found"
                ),
            })?;

        let st_idx = current_max + 1;
        let text_len = ed - st;
        for i in 0..text_len {
            let p = st_idx + i as i64;
            positions.push([p, p, p]);
            current_max = current_max.max(p);
        }

        let llm_t = t as i64;
        let llm_h = (h / spatial_merge_size) as i64;
        let llm_w = (w / spatial_merge_size) as i64;
        let vision_base = st_idx + text_len as i64;

        for _tt in 0..llm_t {
            for hh in 0..llm_h {
                for ww in 0..llm_w {
                    let t_pos = vision_base;
                    let h_pos = vision_base + hh;
                    let w_pos = vision_base + ww;
                    positions.push([t_pos, h_pos, w_pos]);
                    current_max = current_max.max(t_pos).max(h_pos).max(w_pos);
                }
            }
        }

        st = ed + (llm_t as usize) * (llm_h as usize) * (llm_w as usize);
    }

    let st_idx = current_max + 1;
    for i in st..input_ids.len() {
        let p = st_idx + (i - st) as i64;
        positions.push([p, p, p]);
        current_max = p;
    }

    if positions.len() != input_ids.len() {
        return Err(Error::InvalidInput {
            message: format!(
                "PaddleOCR-VL: rope position ids length mismatch: got {}, expected {}",
                positions.len(),
                input_ids.len()
            ),
        });
    }

    let mut pos_ids: Vec<i64> = vec![0; 3 * input_ids.len()];
    let len = input_ids.len();
    for (i, v) in positions.iter().enumerate() {
        pos_ids[i] = v[0];
        pos_ids[len + i] = v[1];
        pos_ids[2 * len + i] = v[2];
    }

    let rope_deltas = (current_max + 1) - (input_ids.len() as i64);

    let position_ids = Tensor::from_vec(pos_ids, (3usize, 1usize, input_ids.len()), device)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: build position_ids tensor failed",
                e,
            )
        })?;

    Ok((position_ids, rope_deltas))
}

/// Require at least 32 repeated tokens and four identical runs. This avoids
/// stopping on short punctuation, tags, or a few legitimate repeated words.
fn repeated_suffix(tokens: &[u32]) -> Option<usize> {
    for width in 1..=64.min(tokens.len() / 4) {
        let repeats = 32usize.div_ceil(width).max(4);
        let span = repeats * width;
        if tokens.len() < span {
            continue;
        }
        let start = tokens.len() - span;
        let pattern = &tokens[start..start + width];
        if tokens[start..]
            .chunks_exact(width)
            .all(|chunk| chunk == pattern)
        {
            return Some(start + width);
        }
    }
    None
}

#[cfg(test)]
mod decode_guard_tests {
    use super::repeated_suffix;
    #[test]
    fn stops_token_runs_without_trimming_short_legitimate_repeats() {
        assert_eq!(repeated_suffix(&[7; 32]), Some(1));
        let repeated: Vec<_> = (0..8).cycle().take(32).collect();
        assert_eq!(repeated_suffix(&repeated), Some(8));
        assert_eq!(repeated_suffix(&[7; 12]), None);
        assert_eq!(repeated_suffix(&(0..128).collect::<Vec<_>>()), None);
    }
}
