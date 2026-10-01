use super::config::{MinerUConfig, MinerUImageProcessorConfig};
use super::processing::preprocess_images;
use super::text::MinerUTextModel;
use super::vision::MinerUVisionModel;
use crate::attention::{
    combine_masks, create_causal_mask, create_generation_mask_if_needed, create_left_padding_mask,
};
#[cfg(feature = "cuda")]
use crate::cuda_kernels::{ArgmaxFirstBf16, ArgmaxFirstF32, MaskTokenIds};
#[cfg(feature = "cuda")]
use crate::decoder_graph::CudaGraphDrainGuard;
use crate::error::Error;
use crate::structure::LayoutElementType;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, VarBuilder, linear_no_bias};
use image::RgbImage;
use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;
use tokenizers::Tokenizer;

/// Canonical MinerU2.5 per-element prompts (mirrors `DEFAULT_PROMPTS` in the
/// official `mineru_vl_utils`). The `two_step_extract` flow routes each cropped
/// region to the matching prompt; a lone `Text Recognition:` prompt over a whole
/// page yields generic markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum MinerUTaskPrompt {
    /// `\nText Recognition:` — default for body text, titles, paragraphs,
    /// lists, captions, references, footnotes, page numbers, etc.
    Text,
    /// `\nFormula Recognition:` — display formulas (`Formula`,
    /// `FormulaNumber`).
    Formula,
    /// `\nTable Recognition:` — tables.
    Table,
    /// `\nImage Analysis:` — figure / image / chart blocks.
    ImageAnalysis,
    /// `\nLayout Detection:` — full-page layout dump (only used by
    /// `two_step_extract` Stage 0). Kept for completeness with the official
    /// `mineru_vl_utils` prompt set so callers can drive the layout pass
    /// externally if they choose.
    #[allow(dead_code)]
    LayoutDetection,
}

#[allow(dead_code)]
impl MinerUTaskPrompt {
    /// Canonical prompt string (with the leading `\n` that MinerU's
    /// `two_step_extract` builds via its chat-template wrapper).
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Text => "\nText Recognition:",
            Self::Formula => "\nFormula Recognition:",
            Self::Table => "\nTable Recognition:",
            Self::ImageAnalysis => "\nImage Analysis:",
            Self::LayoutDetection => "\nLayout Detection:",
        }
    }

    /// Map an OAR `LayoutElementType` to the MinerU element prompt that best
    /// matches its content kind. Mirrors the mapping the official `mineru_vl_utils`
    /// client uses when picking a per-block prompt (`[default]` for text-like blocks,
    /// `table` for tables, `equation` for equations, and `image` for images and charts).
    pub fn for_layout(t: LayoutElementType) -> Self {
        use LayoutElementType::*;
        match t {
            Table => Self::Table,
            Formula | FormulaNumber => Self::Formula,
            Image | Chart | Seal | HeaderImage | FooterImage => Self::ImageAnalysis,
            _ => Self::Text,
        }
    }
}

pub struct MinerU {
    device: Device,
    dtype: DType,
    cfg: MinerUConfig,
    image_cfg: MinerUImageProcessorConfig,
    tokenizer: Tokenizer,
    text: MinerUTextModel,
    vision: MinerUVisionModel,
    lm_head: Linear,
    image_token_id: u32,
    eos_token_ids: Vec<u32>,
    skip_token_ids: HashSet<u32>,
    vision_start_token_id: u32,
    video_token_id: u32,
    spatial_merge_size: usize,
    repetition_penalty: f32,
    no_repeat_ngram_size: usize,
    do_sample: bool,
    temperature: f32,
    top_p: f32,
    top_k: usize,
    #[cfg(feature = "cuda")]
    gpu_greedy_sampling: bool,
    // Must stay the last field: the captured decoder graphs read the LM head
    // owned here, so this guard drops last and drains CUDA errors the head's
    // free may stash (see CudaGraphDrainGuard).
    #[cfg(feature = "cuda")]
    _drain_guard: CudaGraphDrainGuard,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MinerUEosTokenId {
    Single(u32),
    Multi(Vec<u32>),
}

#[derive(Debug, Deserialize)]
struct MinerUGenerationConfig {
    #[serde(default)]
    do_sample: Option<bool>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    top_k: Option<u32>,
    #[serde(default)]
    repetition_penalty: Option<f32>,
    #[serde(default)]
    no_repeat_ngram_size: Option<usize>,
    #[serde(default)]
    eos_token_id: Option<MinerUEosTokenId>,
    #[serde(default)]
    pad_token_id: Option<u32>,
}

impl MinerU {
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = MinerUConfig::from_path(model_dir.join("config.json"))?;
        let image_cfg =
            MinerUImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;

        if image_cfg.merge_size != cfg.vision_config.spatial_merge_size {
            return Err(Error::Config {
                message: format!(
                    "MinerU2.5 merge_size mismatch: preprocessor {} != vision {}",
                    image_cfg.merge_size, cfg.vision_config.spatial_merge_size
                ),
            });
        }
        if image_cfg.patch_size != cfg.vision_config.patch_size {
            return Err(Error::Config {
                message: format!(
                    "MinerU2.5 patch_size mismatch: preprocessor {} != vision {}",
                    image_cfg.patch_size, cfg.vision_config.patch_size
                ),
            });
        }

        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("failed to load MinerU2.5 tokenizer.json: {e}"),
            })?;

        let gen_cfg = load_generation_config(model_dir.join("generation_config.json"));
        let repetition_penalty = gen_cfg
            .as_ref()
            .and_then(|cfg| cfg.repetition_penalty)
            .unwrap_or(1.0);
        // generation_config.json ships no `no_repeat_ngram_size`, but the
        // official two-step extraction (mineru_vl_utils) sets it to 100 for the
        // default/table/formula recognition passes, which is exactly how this
        // model is driven here. Match that default when the config is silent.
        let no_repeat_ngram_size = gen_cfg
            .as_ref()
            .and_then(|cfg| cfg.no_repeat_ngram_size)
            .unwrap_or(100);
        let do_sample = gen_cfg
            .as_ref()
            .and_then(|cfg| cfg.do_sample)
            .unwrap_or(false);
        let temperature = gen_cfg
            .as_ref()
            .and_then(|cfg| cfg.temperature)
            .unwrap_or(1.0);
        let top_p = gen_cfg.as_ref().and_then(|cfg| cfg.top_p).unwrap_or(1.0);
        let top_k = gen_cfg
            .as_ref()
            .and_then(|cfg| cfg.top_k)
            .map(|v| v as usize)
            .unwrap_or(0);

        if let Some(tok_image_id) = tokenizer.token_to_id("<|image_pad|>")
            && tok_image_id != cfg.image_token_id
        {
            return Err(Error::Config {
                message: format!(
                    "MinerU2.5 image_token_id mismatch: tokenizer {tok_image_id} != config {}",
                    cfg.image_token_id
                ),
            });
        }

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, "MinerU2.5")?;
        // SAFETY: from_mmaped_safetensors memory-maps the weight files directly;
        // the caller must ensure they are valid and not modified while in use.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load safetensors", e))?
        };

        let text = MinerUTextModel::load(&cfg, vb.pp("model"))?;
        let vision = MinerUVisionModel::load(&cfg.vision_config, vb.pp("visual"))?;

        let image_token_id = cfg.image_token_id;
        let mut eos_token_ids = vec![cfg.eos_token_id];
        if let Some(gen_cfg) = &gen_cfg {
            if let Some(eos) = &gen_cfg.eos_token_id {
                match eos {
                    MinerUEosTokenId::Single(id) => eos_token_ids.push(*id),
                    MinerUEosTokenId::Multi(ids) => eos_token_ids.extend(ids.iter().copied()),
                }
            }
            if let Some(pad) = gen_cfg.pad_token_id {
                eos_token_ids.push(pad);
            }
        }
        eos_token_ids.sort_unstable();
        eos_token_ids.dedup();

        // Build skip_token_ids set (bos, eos, pad) for filtering before decode
        let mut skip_token_ids: HashSet<u32> = HashSet::new();
        skip_token_ids.insert(cfg.bos_token_id);
        skip_token_ids.extend(eos_token_ids.iter().copied());
        if let Some(pad) = cfg.pad_token_id {
            skip_token_ids.insert(pad);
        }

        let vision_start_token_id = cfg.vision_start_token_id;
        let video_token_id = cfg.video_token_id;
        let spatial_merge_size = cfg.vision_config.spatial_merge_size;

        let lm_head = if cfg.tie_word_embeddings() {
            Linear::new(text.token_embedding_weight(), None)
        } else {
            linear_no_bias(cfg.hidden_size, cfg.vocab_size, vb.pp("lm_head"))
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load lm_head", e))?
        };

        #[cfg(feature = "cuda")]
        let _drain_guard = CudaGraphDrainGuard::new(&device);
        Ok(Self {
            device,
            dtype,
            cfg,
            image_cfg,
            tokenizer,
            text,
            vision,
            lm_head,
            image_token_id,
            eos_token_ids,
            skip_token_ids,
            vision_start_token_id,
            video_token_id,
            spatial_merge_size,
            repetition_penalty,
            no_repeat_ngram_size,
            do_sample,
            temperature,
            top_p,
            top_k,
            #[cfg(feature = "cuda")]
            gpu_greedy_sampling: std::env::var_os("OAR_MINERU_DISABLE_GPU_SAMPLING").is_none(),
            #[cfg(feature = "cuda")]
            _drain_guard,
        })
    }

    /// Generate text for a batch of images and instructions.
    ///
    /// # Arguments
    /// * `images` - Input images to process
    /// * `instructions` - Text instructions/prompts for each image
    /// * `max_new_tokens` - Maximum number of new tokens to generate
    ///
    /// # Returns
    /// Vector of results, one for each input image-instruction pair
    ///
    /// Unequal prompt lengths are left-padded. Both prefill and incremental
    /// decode mask those padding KV positions, so mixed image sizes remain
    /// correct in the same batch.
    pub fn generate(
        &self,
        images: &[RgbImage],
        instructions: &[impl AsRef<str>],
        max_new_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        if images.len() != instructions.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU2.5: images count ({}) != instructions count ({})",
                    images.len(),
                    instructions.len()
                ),
            });
        }

        let results = self.generate_tokens_internal(images, instructions, max_new_tokens)?;
        Ok(results
            .into_iter()
            .map(|tokens| self.decode_generated_tokens(&tokens))
            .collect())
    }

    /// Generate raw baseline tokens for oracle-draft / tokenizer round-trip
    /// experiments. Tokens are exactly the ids emitted by the decode loop,
    /// excluding stop tokens, before skip-token filtering and tokenizer decode.
    pub fn generate_tokens(
        &self,
        images: &[RgbImage],
        instructions: &[impl AsRef<str>],
        max_new_tokens: usize,
    ) -> Result<Vec<Vec<u32>>, Error> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        if images.len() != instructions.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU2.5: images count ({}) != instructions count ({})",
                    images.len(),
                    instructions.len()
                ),
            });
        }
        self.generate_tokens_internal(images, instructions, max_new_tokens)
    }

    fn generate_tokens_internal(
        &self,
        images: &[RgbImage],
        instructions: &[impl AsRef<str>],
        max_new_tokens: usize,
    ) -> Result<Vec<Vec<u32>>, Error> {
        let batch_size = images.len();

        let image_inputs = preprocess_images(images, &self.image_cfg, &self.device, self.dtype)?;
        let image_token_counts: Vec<usize> = image_inputs
            .image_grid_thw
            .iter()
            .map(|&(t, h, w)| {
                let denom = self.spatial_merge_size * self.spatial_merge_size;
                (t * h * w) / denom
            })
            .collect();

        let mut all_input_ids: Vec<Vec<u32>> = Vec::with_capacity(batch_size);

        for (instruction, &image_token_count) in instructions.iter().zip(image_token_counts.iter())
        {
            let prompt = build_prompt(instruction.as_ref());
            let enc = self
                .tokenizer
                .encode(prompt, false)
                .map_err(|e| Error::InvalidInput {
                    message: format!("MinerU2.5: tokenizer encode failed: {e}"),
                })?;

            let input_ids =
                expand_image_tokens(enc.get_ids(), self.image_token_id, &[image_token_count])?;
            all_input_ids.push(input_ids);
        }

        let image_embeds_all = self
            .vision
            .forward(&image_inputs.pixel_values, &image_inputs.image_grid_thw)?;
        let expected_embeds: usize = image_token_counts.iter().sum();
        let actual_embeds = image_embeds_all
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "image_embeds dim", e))?;
        if actual_embeds != expected_embeds {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU2.5: image embeds count mismatch: got {actual_embeds}, expected {expected_embeds}"
                ),
            });
        }

        let seq_lens: Vec<usize> = all_input_ids.iter().map(|ids| ids.len()).collect();
        let Some(&max_seq_len) = seq_lens.iter().max() else {
            return Err(Error::InvalidInput {
                message: "MinerU2.5: empty batch is not supported".to_string(),
            });
        };

        let mut batch_embeds: Vec<Tensor> = Vec::with_capacity(batch_size);
        let mut rope_deltas: Vec<i64> = Vec::with_capacity(batch_size);
        let mut batch_position_ids: Vec<Tensor> = Vec::with_capacity(batch_size);
        let mut embed_offset = 0usize;
        let mut history_tokens: Vec<Vec<u32>> = all_input_ids.clone();

        for (i, input_ids) in all_input_ids.iter().enumerate() {
            let seq_len = input_ids.len();
            let pad_len = max_seq_len - seq_len;
            let image_token_count = image_token_counts[i];

            let image_embeds = image_embeds_all
                .narrow(0, embed_offset, image_token_count)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "narrow image embeds", e))?;
            embed_offset += image_token_count;

            let input_ids_t = Tensor::new(input_ids.clone(), &self.device)
                .and_then(|t| t.reshape((1, seq_len)))
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create input_ids", e))?;
            let mut inputs_embeds = self.text.embed(&input_ids_t)?;

            let first_img_pos = input_ids.iter().position(|&id| id == self.image_token_id);
            if let Some(first_pos) = first_img_pos {
                let image_end = first_pos + image_token_count;
                if image_end > seq_len {
                    return Err(Error::InvalidInput {
                        message: format!(
                            "MinerU2.5: image token span out of range: {image_end} > {seq_len}"
                        ),
                    });
                }
                let mut parts: Vec<Tensor> = Vec::with_capacity(3);
                if first_pos > 0 {
                    parts.push(
                        inputs_embeds.narrow(1, 0, first_pos).map_err(|e| {
                            candle_to_ocr_inference("MinerU2.5", "narrow prefix", e)
                        })?,
                    );
                }
                parts.push(
                    image_embeds
                        .unsqueeze(0)
                        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "unsqueeze img", e))?,
                );
                if image_end < seq_len {
                    parts.push(
                        inputs_embeds
                            .narrow(1, image_end, seq_len - image_end)
                            .map_err(|e| {
                                candle_to_ocr_inference("MinerU2.5", "narrow suffix", e)
                            })?,
                    );
                }

                let refs: Vec<&Tensor> = parts.iter().collect();
                inputs_embeds = Tensor::cat(&refs, 1)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "cat embeds", e))?;
            }

            if pad_len > 0 {
                let pad = Tensor::zeros(
                    (1, pad_len, self.cfg.hidden_size),
                    inputs_embeds.dtype(),
                    &self.device,
                )
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create pad", e))?;
                inputs_embeds = Tensor::cat(&[&pad, &inputs_embeds], 1)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "cat pad", e))?;
            }
            batch_embeds.push(inputs_embeds);

            let (pos_ids, delta) = get_rope_index(
                &self.cfg,
                input_ids,
                &[image_inputs.image_grid_thw[i]],
                self.vision_start_token_id,
                self.video_token_id,
                self.spatial_merge_size,
                &self.device,
            )?;
            rope_deltas.push(delta);

            let pos_ids = if pad_len > 0 {
                let pad_pos = Tensor::zeros((3, 1, pad_len), DType::I64, &self.device)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create pad pos", e))?;
                Tensor::cat(&[&pad_pos, &pos_ids], 2)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "cat pad pos", e))?
            } else {
                pos_ids
            };
            batch_position_ids.push(pos_ids);
        }

        let batch_refs: Vec<&Tensor> = batch_embeds.iter().collect();
        let inputs_embeds = Tensor::cat(&batch_refs, 0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "stack embeds", e))?;

        let pos_refs: Vec<&Tensor> = batch_position_ids.iter().collect();
        let position_ids = Tensor::cat(&pos_refs, 1)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "stack pos", e))?;

        let mask = if batch_size == 1 {
            None
        } else {
            let causal = create_causal_mask(max_seq_len, max_seq_len, self.dtype, &self.device)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create causal", e))?;
            let padding =
                create_left_padding_mask(&seq_lens, max_seq_len, self.dtype, &self.device)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create padding", e))?;
            Some(
                combine_masks(&causal, &padding)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "combine masks", e))?,
            )
        };

        self.text.clear_kv_cache();
        if batch_size == 1 {
            self.text
                .prepare_ar_cuda_graph(max_seq_len, max_new_tokens, &self.lm_head)?;
        } else {
            // Batch-shaped prefill replaces the batch-1 KV backing storage.
            // Drop the captured graph before those raw pointers become stale.
            self.text.invalidate_ar_cuda_graph();
        }
        let hidden = self
            .text
            .forward(&inputs_embeds, &position_ids, mask.as_ref())?;

        let last_hidden = hidden
            .i((.., max_seq_len - 1, ..))
            .and_then(|hidden| hidden.contiguous())
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "get last hidden", e))?;
        let batched_logits = self
            .lm_head
            .forward(&last_hidden)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "lm_head", e))?;
        let mut logits_list: Vec<Tensor> = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            logits_list.push(
                batched_logits
                    .i(i)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "select logits", e))?,
            );
        }

        let mut generated: Vec<Vec<u32>> = vec![Vec::new(); batch_size];
        let mut finished: Vec<bool> = vec![false; batch_size];
        let mut positions: Vec<i64> = seq_lens
            .iter()
            .zip(&rope_deltas)
            .map(|(&len, &d)| (len as i64) + d)
            .collect();

        // Track padding lengths so incremental decode cannot attend to the
        // leading padding KV of shorter prompts.
        let pad_lens: Vec<usize> = seq_lens.iter().map(|&len| max_seq_len - len).collect();
        // Current KV cache length (grows during generation)
        let mut kv_len = max_seq_len;

        for step in 0..max_new_tokens {
            if finished.iter().all(|&f| f) {
                break;
            }
            let sampling_params = self.sampling_params();
            let mut next_tokens: Vec<u32> = Vec::with_capacity(batch_size);
            for (i, logits) in logits_list.iter().enumerate() {
                if finished[i] {
                    next_tokens.push(0);
                } else {
                    let tok = select_next_token(logits, &history_tokens[i], &sampling_params)?;
                    if self.eos_token_ids.contains(&tok) {
                        finished[i] = true;
                    } else {
                        generated[i].push(tok);
                        history_tokens[i].push(tok);
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

            let tokens = Tensor::new(next_tokens, &self.device)
                .and_then(|t| t.reshape((batch_size, 1)))
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create tokens", e))?;
            let embeds = self.text.embed(&tokens)?;

            let pos_data = crate::attention::decode_position_buffer(&positions, 3);
            let pos = Tensor::new(pos_data, &self.device)
                .and_then(|t| t.reshape((3, batch_size, 1)))
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create pos", e))?;

            // Create attention mask for generation step:
            // - Query position can attend to all non-padding positions in KV cache
            // - Padding positions (first pad_len tokens) should be masked
            kv_len += 1;
            let gen_mask = if batch_size == 1 {
                None
            } else {
                create_generation_mask_if_needed(&pad_lens, kv_len, self.dtype, &self.device)
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "create gen mask", e))?
            };

            logits_list.clear();
            if batch_size == 1 {
                logits_list.push(self.text.forward_decode_logits(
                    &embeds,
                    &pos,
                    gen_mask.as_ref(),
                    &self.lm_head,
                )?);
            } else {
                let hs = self.text.forward(&embeds, &pos, gen_mask.as_ref())?;
                let batched_logits = self
                    .lm_head
                    .forward(&hs)
                    .and_then(|t| t.squeeze(1))
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "lm_head step", e))?;
                for i in 0..batch_size {
                    logits_list.push(batched_logits.i(i).map_err(|e| {
                        candle_to_ocr_inference("MinerU2.5", "select step logits", e)
                    })?);
                }
            }

            for (i, p) in positions.iter_mut().enumerate() {
                if !finished[i] {
                    *p += 1;
                }
            }
        }

        Ok(generated)
    }

    pub fn decode_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        self.decode_generated_tokens(tokens)
    }

    /// Decode tokens in the form the model actually emitted. MinerU2.5's
    /// `decode_tokens` only filters bos/eos/pad before `tokenizer.decode` —
    /// there is no markdown / wrapping / layout post-process at this layer
    /// (layout-aware reordering happens in `two_step_extract`, not here).
    /// This alias exists for API symmetry with PaddleOCR-VL / GLM-OCR.
    pub fn decode_tokens_raw(&self, tokens: &[u32]) -> Result<String, Error> {
        self.decode_generated_tokens(tokens)
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    fn sampling_params(&self) -> SamplingParams {
        SamplingParams {
            repetition_penalty: self.repetition_penalty,
            no_repeat_ngram_size: self.no_repeat_ngram_size,
            do_sample: self.do_sample,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            #[cfg(feature = "cuda")]
            gpu_greedy_sampling: self.gpu_greedy_sampling,
        }
    }

    fn decode_generated_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        // Filter out bos/eos/pad tokens before decoding (matching official implementation).
        let filtered: Vec<u32> = tokens
            .iter()
            .copied()
            .filter(|t| !self.skip_token_ids.contains(t))
            .collect();
        self.tokenizer
            .decode(&filtered, false) // skip_special_tokens=false to preserve special tokens
            .map_err(|e| Error::InvalidInput {
                message: format!("decode failed: {e}"),
            })
    }
}

fn build_prompt(instruction: &str) -> String {
    let separator = if instruction.starts_with(' ') || instruction.starts_with('\n') {
        ""
    } else {
        " "
    };
    format!(
        "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n<|vision_start|><|image_pad|><|vision_end|>{separator}{instruction}<|im_end|>\n<|im_start|>assistant\n"
    )
}

fn load_generation_config(path: impl AsRef<Path>) -> Option<MinerUGenerationConfig> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

struct SamplingParams {
    repetition_penalty: f32,
    no_repeat_ngram_size: usize,
    do_sample: bool,
    temperature: f32,
    top_p: f32,
    top_k: usize,
    #[cfg(feature = "cuda")]
    gpu_greedy_sampling: bool,
}

impl SamplingParams {
    fn is_greedy(&self) -> bool {
        !self.do_sample || self.top_k == 1
    }
}

fn select_next_token(
    logits: &Tensor,
    history: &[u32],
    params: &SamplingParams,
) -> Result<u32, Error> {
    // The official MinerU generation config uses top_k=1 and
    // repetition_penalty=1, so decoding is greedy even though do_sample=true.
    // Keep the full vocabulary on-device for that common path. The custom
    // reduction preserves the CPU loop's lowest-index tie break, and the
    // sparse mask retains no-repeat-ngram semantics exactly.
    #[cfg(feature = "cuda")]
    if params.gpu_greedy_sampling
        && logits.device().is_cuda()
        && params.is_greedy()
        && params.repetition_penalty <= 1.0
        && matches!(logits.dtype(), DType::BF16 | DType::F32)
    {
        return select_greedy_token_cuda(logits, history, params.no_repeat_ngram_size);
    }

    // Metal's generic argmax returns a scalar token without downloading and
    // allocating the full vocabulary on the host. Preserve the CPU processor
    // path whenever repetition or no-repeat-ngram would modify the logits.
    if logits.device().is_metal()
        && params.is_greedy()
        && params.repetition_penalty <= 1.0
        && matches!(logits.dtype(), DType::F16 | DType::BF16 | DType::F32)
        && no_repeat_ngram_banned_tokens(history, params.no_repeat_ngram_size).is_empty()
    {
        let not_nan = logits
            .eq(logits)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "Metal NaN mask", e))?;
        let neg_inf = Tensor::new(f32::NEG_INFINITY, logits.device())
            .and_then(|value| value.to_dtype(logits.dtype()))
            .and_then(|value| value.broadcast_as(logits.dims()))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "Metal NaN replacement", e))?;
        return not_nan
            .where_cond(logits, &neg_inf)
            .and_then(|logits| logits.argmax(candle_core::D::Minus1))
            .and_then(|token| token.to_scalar::<u32>())
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "Metal greedy argmax", e));
    }

    let logits = logits
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "logits cast", e))?
        .to_device(&Device::Cpu)
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "logits to cpu", e))?;
    let mut logits_vec = logits
        .to_vec1::<f32>()
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "logits to vec", e))?;

    apply_sampling_processors(&mut logits_vec, history, params);

    if params.is_greedy() {
        return Ok(argmax_token(&logits_vec));
    }

    let probs = softmax(&logits_vec);
    if let Some(idx) = sample_from_probs(&probs) {
        Ok(idx as u32)
    } else {
        Ok(argmax_token(&logits_vec))
    }
}

#[cfg(feature = "cuda")]
fn select_greedy_token_cuda(
    logits: &Tensor,
    history: &[u32],
    no_repeat_ngram_size: usize,
) -> Result<u32, Error> {
    let vocab_size = logits.elem_count();
    let logits = logits
        .reshape((1, vocab_size))
        .and_then(|logits| logits.contiguous())
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "reshape GPU logits", e))?;
    let banned = no_repeat_ngram_banned_tokens(history, no_repeat_ngram_size);
    if !banned.is_empty() {
        let banned = Tensor::new(banned, logits.device())
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "upload banned token ids", e))?;
        logits.inplace_op2(&banned, &MaskTokenIds).map_err(|e| {
            candle_to_ocr_inference("MinerU2.5", "apply GPU no-repeat-ngram mask", e)
        })?;
    }
    let tokens = match logits.dtype() {
        DType::BF16 => logits.apply_op1_no_bwd(&ArgmaxFirstBf16),
        DType::F32 => logits.apply_op1_no_bwd(&ArgmaxFirstF32),
        dtype => {
            return Err(Error::Config {
                message: format!("MinerU2.5: unsupported GPU greedy logits dtype {dtype:?}"),
            });
        }
    }
    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "stable GPU argmax", e))?;
    tokens
        .i(0)
        .and_then(|token| token.to_scalar::<u32>())
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "copy selected token", e))
}

fn apply_sampling_processors(logits: &mut [f32], history: &[u32], params: &SamplingParams) {
    apply_repetition_penalty(logits, history, params.repetition_penalty);
    apply_no_repeat_ngram(logits, history, params.no_repeat_ngram_size);

    if params.is_greedy() {
        return;
    }

    let temp = if params.temperature <= 0.0 {
        1.0
    } else {
        params.temperature
    };
    if (temp - 1.0).abs() > f32::EPSILON {
        for val in logits.iter_mut() {
            *val /= temp;
        }
    }

    apply_top_k(logits, params.top_k);
    apply_top_p(logits, params.top_p);
}

fn argmax_token(logits: &[f32]) -> u32 {
    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (idx, &val) in logits.iter().enumerate() {
        if val.is_nan() {
            continue;
        }
        if val > best_val {
            best_val = val;
            best_idx = idx;
        }
    }
    best_idx as u32
}

fn apply_repetition_penalty(logits: &mut [f32], history: &[u32], penalty: f32) {
    if penalty <= 1.0 {
        return;
    }
    let mut seen = HashSet::new();
    for &token in history {
        if !seen.insert(token) {
            continue;
        }
        let idx = token as usize;
        if idx >= logits.len() {
            continue;
        }
        let val = logits[idx];
        logits[idx] = if val < 0.0 {
            val * penalty
        } else {
            val / penalty
        };
    }
}

fn apply_top_k(logits: &mut [f32], top_k: usize) {
    if top_k == 0 || top_k >= logits.len() {
        return;
    }
    let mut indices: Vec<usize> = (0..logits.len()).collect();
    indices.sort_by(|&a, &b| logits[b].partial_cmp(&logits[a]).unwrap_or(Ordering::Less));
    for &idx in indices.iter().skip(top_k) {
        logits[idx] = f32::NEG_INFINITY;
    }
}

fn apply_top_p(logits: &mut [f32], top_p: f32) {
    if !(0.0..1.0).contains(&top_p) {
        return;
    }
    let mut indices: Vec<usize> = (0..logits.len()).collect();
    indices.sort_by(|&a, &b| logits[b].partial_cmp(&logits[a]).unwrap_or(Ordering::Less));

    let max = logits[indices[0]];
    let mut exp_vals: Vec<f32> = Vec::with_capacity(indices.len());
    let mut exp_sum = 0.0f32;
    for &idx in &indices {
        let val = logits[idx];
        let exp = if val.is_finite() {
            (val - max).exp()
        } else {
            0.0
        };
        exp_vals.push(exp);
        exp_sum += exp;
    }
    if exp_sum == 0.0 {
        return;
    }

    let mut cumulative = 0.0f32;
    for (rank, _) in indices.iter().enumerate() {
        let prob = exp_vals[rank] / exp_sum;
        cumulative += prob;
        if cumulative > top_p && rank > 0 {
            for &drop in indices.iter().skip(rank) {
                logits[drop] = f32::NEG_INFINITY;
            }
            break;
        }
    }
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let mut max = f32::NEG_INFINITY;
    for &val in logits {
        if val.is_finite() && val > max {
            max = val;
        }
    }
    let mut exps = Vec::with_capacity(logits.len());
    let mut sum = 0.0f32;
    for &val in logits {
        let exp = if val.is_finite() {
            (val - max).exp()
        } else {
            0.0
        };
        exps.push(exp);
        sum += exp;
    }
    if sum == 0.0 {
        return vec![0.0; logits.len()];
    }
    exps.into_iter().map(|v| v / sum).collect()
}

fn sample_from_probs(probs: &[f32]) -> Option<usize> {
    let dist = WeightedIndex::new(probs).ok()?;
    let mut rng = rand::rng();
    Some(dist.sample(&mut rng))
}

fn apply_no_repeat_ngram(logits: &mut [f32], history: &[u32], ngram_size: usize) {
    for token in no_repeat_ngram_banned_tokens(history, ngram_size) {
        let idx = token as usize;
        if idx < logits.len() {
            logits[idx] = f32::NEG_INFINITY;
        }
    }
}

fn no_repeat_ngram_banned_tokens(history: &[u32], ngram_size: usize) -> Vec<u32> {
    if ngram_size <= 1 || history.len() < ngram_size {
        return Vec::new();
    }
    let prefix_len = ngram_size - 1;
    let prefix_start = history.len() - prefix_len;
    let prefix = &history[prefix_start..];
    let mut banned = HashSet::new();
    for i in 0..=history.len() - ngram_size {
        if history[i..i + prefix_len] == *prefix {
            banned.insert(history[i + prefix_len]);
        }
    }
    let mut banned: Vec<u32> = banned.into_iter().collect();
    banned.sort_unstable();
    banned
}

fn expand_image_tokens(
    input_ids: &[u32],
    image_token_id: u32,
    image_token_counts: &[usize],
) -> Result<Vec<u32>, Error> {
    let mut out: Vec<u32> = Vec::new();
    let mut idx = 0usize;
    for &id in input_ids {
        if id == image_token_id {
            let count = image_token_counts
                .get(idx)
                .ok_or_else(|| Error::InvalidInput {
                    message: "MinerU2.5: image token count mismatch".to_string(),
                })?;
            out.extend(std::iter::repeat_n(image_token_id, *count));
            idx += 1;
        } else {
            out.push(id);
        }
    }
    if idx != image_token_counts.len() {
        return Err(Error::InvalidInput {
            message: "MinerU2.5: image token count mismatch".to_string(),
        });
    }
    Ok(out)
}

fn get_rope_index(
    cfg: &MinerUConfig,
    input_ids: &[u32],
    image_grid_thw: &[(usize, usize, usize)],
    vision_start_token_id: u32,
    video_token_id: u32,
    spatial_merge_size: usize,
    device: &Device,
) -> Result<(Tensor, i64), Error> {
    // Qwen2-VL multimodal RoPE has three position axes: temporal, height, width.
    const NUM_ROPE_AXES: usize = 3;
    let image_token_id = cfg.image_token_id;
    let mut image_count = 0usize;
    for i in 0..input_ids.len().saturating_sub(1) {
        if input_ids[i] == vision_start_token_id && input_ids[i + 1] == image_token_id {
            image_count += 1;
        }
        if input_ids[i] == vision_start_token_id && input_ids[i + 1] == video_token_id {
            return Err(Error::InvalidInput {
                message: "MinerU2.5: video inputs are not supported".to_string(),
            });
        }
    }
    if image_count != image_grid_thw.len() {
        return Err(Error::InvalidInput {
            message: format!(
                "MinerU2.5: image count mismatch between prompt ({image_count}) and image_grid_thw ({})",
                image_grid_thw.len()
            ),
        });
    }

    let mut positions: Vec<[i64; 3]> = Vec::with_capacity(input_ids.len());
    let mut st = 0usize;
    let mut current_max: i64 = -1;

    for (image_index, &(t, h, w)) in image_grid_thw.iter().enumerate().take(image_count) {
        let ed = input_ids[st..]
            .iter()
            .position(|&id| id == image_token_id)
            .map(|p| st + p)
            .ok_or_else(|| Error::InvalidInput {
                message: format!(
                    "MinerU2.5: expected image token for image[{image_index}] but none found"
                ),
            })?;

        let st_idx = if current_max >= 0 { current_max + 1 } else { 0 };
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

        for tt in 0..llm_t {
            for hh in 0..llm_h {
                for ww in 0..llm_w {
                    let t_pos = vision_base + tt;
                    let h_pos = vision_base + hh;
                    let w_pos = vision_base + ww;
                    positions.push([t_pos, h_pos, w_pos]);
                    current_max = current_max.max(t_pos).max(h_pos).max(w_pos);
                }
            }
        }

        st = ed + (llm_t as usize) * (llm_h as usize) * (llm_w as usize);
    }

    let st_idx = if current_max >= 0 { current_max + 1 } else { 0 };
    for i in st..input_ids.len() {
        let p = st_idx + (i - st) as i64;
        positions.push([p, p, p]);
        current_max = p;
    }

    if positions.len() != input_ids.len() {
        return Err(Error::InvalidInput {
            message: format!(
                "MinerU2.5: rope position ids length mismatch: got {}, expected {}",
                positions.len(),
                input_ids.len()
            ),
        });
    }

    let mut pos_ids: Vec<i64> = vec![0; NUM_ROPE_AXES * input_ids.len()];
    let len = input_ids.len();
    for (i, v) in positions.iter().enumerate() {
        pos_ids[i] = v[0];
        pos_ids[len + i] = v[1];
        pos_ids[2 * len + i] = v[2];
    }

    let rope_delta = (current_max + 1) - (input_ids.len() as i64);

    let position_ids = Tensor::from_vec(pos_ids, (NUM_ROPE_AXES, 1usize, input_ids.len()), device)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: build position_ids tensor failed",
                e,
            )
        })?;

    Ok((position_ids, rope_delta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_repeat_ngram_collects_all_matching_continuations() {
        assert_eq!(
            no_repeat_ngram_banned_tokens(&[1, 2, 3, 1, 2, 4, 1, 2], 3),
            vec![3, 4]
        );
        assert!(no_repeat_ngram_banned_tokens(&[1, 2], 3).is_empty());
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn mineru_cuda_greedy_matches_cpu_ties_and_no_repeat_ngram() -> candle_core::Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        let mut values = vec![0.0f32; 1030];
        values[1] = 10.0;
        values[1024] = 10.0;
        let gpu_logits = Tensor::from_vec(values, 1030, &device)?.to_dtype(DType::BF16)?;
        let cpu_logits = gpu_logits.to_device(&Device::Cpu)?;
        let params = SamplingParams {
            repetition_penalty: 1.0,
            no_repeat_ngram_size: 3,
            do_sample: true,
            temperature: 0.01,
            top_p: 0.001,
            top_k: 1,
            gpu_greedy_sampling: true,
        };
        let history = [4, 5, 1, 8, 4, 5];
        let cpu = select_next_token(&cpu_logits, &history, &params).unwrap();
        let gpu = select_next_token(&gpu_logits, &history, &params).unwrap();
        assert_eq!(cpu, 1024);
        assert_eq!(gpu, cpu);
        Ok(())
    }

    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn mineru_metal_greedy_matches_cpu_and_preserves_first_tie() -> candle_core::Result<()> {
        let Ok(device) = Device::new_metal(0) else {
            return Ok(());
        };
        let mut values = vec![0.0f32; 151_936];
        values[0] = f32::NAN;
        values[1] = 10.0;
        values[1024] = 10.0;
        let metal_logits = Tensor::from_vec(values, 151_936, &device)?.to_dtype(DType::F16)?;
        let cpu_logits = metal_logits.to_device(&Device::Cpu)?;
        let params = SamplingParams {
            repetition_penalty: 1.0,
            no_repeat_ngram_size: 3,
            do_sample: true,
            temperature: 0.01,
            top_p: 0.001,
            top_k: 1,
            #[cfg(feature = "cuda")]
            gpu_greedy_sampling: false,
        };

        let cpu = select_next_token(&cpu_logits, &[], &params).unwrap();
        let metal = select_next_token(&metal_logits, &[], &params).unwrap();
        assert_eq!(cpu, 1);
        assert_eq!(metal, cpu);

        // The current bigram [4, 5] bans token 1, forcing the exact CPU
        // processor fallback and selecting the other tied token.
        let history = [4, 5, 1, 8, 4, 5];
        assert_eq!(
            select_next_token(&metal_logits, &history, &params).unwrap(),
            1024
        );
        Ok(())
    }

    #[test]
    fn mineru_task_prompt_text_recognition_matches_official() {
        assert_eq!(MinerUTaskPrompt::Text.prompt(), "\nText Recognition:");
    }

    #[test]
    fn mineru_task_prompt_formula_recognition_matches_official() {
        assert_eq!(MinerUTaskPrompt::Formula.prompt(), "\nFormula Recognition:");
    }

    #[test]
    fn mineru_task_prompt_table_recognition_matches_official() {
        assert_eq!(MinerUTaskPrompt::Table.prompt(), "\nTable Recognition:");
    }

    #[test]
    fn mineru_task_prompt_image_analysis_matches_official() {
        assert_eq!(
            MinerUTaskPrompt::ImageAnalysis.prompt(),
            "\nImage Analysis:"
        );
    }

    #[test]
    fn mineru_task_prompt_layout_detection_matches_official() {
        assert_eq!(
            MinerUTaskPrompt::LayoutDetection.prompt(),
            "\nLayout Detection:"
        );
    }

    #[test]
    fn for_layout_routes_table_kinds_to_table() {
        assert_eq!(
            MinerUTaskPrompt::for_layout(LayoutElementType::Table),
            MinerUTaskPrompt::Table
        );
    }

    #[test]
    fn for_layout_routes_formula_kinds_to_formula() {
        assert_eq!(
            MinerUTaskPrompt::for_layout(LayoutElementType::Formula),
            MinerUTaskPrompt::Formula
        );
        assert_eq!(
            MinerUTaskPrompt::for_layout(LayoutElementType::FormulaNumber),
            MinerUTaskPrompt::Formula
        );
    }

    #[test]
    fn for_layout_routes_visual_kinds_to_image_analysis() {
        for ty in [
            LayoutElementType::Image,
            LayoutElementType::Chart,
            LayoutElementType::Seal,
            LayoutElementType::HeaderImage,
            LayoutElementType::FooterImage,
        ] {
            assert_eq!(
                MinerUTaskPrompt::for_layout(ty),
                MinerUTaskPrompt::ImageAnalysis,
                "expected ImageAnalysis for {ty:?}",
            );
        }
    }

    #[test]
    fn for_layout_defaults_text_for_text_like_kinds() {
        for ty in [
            LayoutElementType::Text,
            LayoutElementType::Content,
            LayoutElementType::DocTitle,
            LayoutElementType::ParagraphTitle,
            LayoutElementType::List,
            LayoutElementType::Reference,
            LayoutElementType::Footnote,
            LayoutElementType::Number,
            LayoutElementType::Header,
            LayoutElementType::Footer,
        ] {
            assert_eq!(
                MinerUTaskPrompt::for_layout(ty),
                MinerUTaskPrompt::Text,
                "expected Text for {ty:?}",
            );
        }
    }
}
