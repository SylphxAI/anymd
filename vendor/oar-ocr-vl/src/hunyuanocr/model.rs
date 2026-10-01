//! HunyuanOCR model wrapper (HunYuanVLForConditionalGeneration).

use super::config::{HunyuanOcrConfig, HunyuanOcrImageProcessorConfig, HunyuanOcrVersion};
use super::dflash::DFlashModel;
#[cfg(feature = "cuda")]
use super::dynamic_kv::{
    DFlashRepetitionArgmaxBf16, MarkRepetitionHistoryU8, RepetitionArgmaxBf16, RepetitionPenaltyF32,
};
use super::llm::HunyuanLlm;
use super::processing::{HunyuanOcrImageInputs, preprocess_image};
use super::vision::HunyuanVisionModel;
use crate::attention::{
    combine_masks, create_causal_mask, create_generation_mask_if_needed, create_left_padding_mask,
};
#[cfg(feature = "cuda")]
use crate::cuda_kernels::ArgmaxFirstF32;
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use image::RgbImage;
use std::path::Path;
use tokenizers::Tokenizer;

/// Read `generation_config.json::repetition_penalty`, using the checkpoint
/// generation's reference fallback when the field is absent. V1.0 ships 1.03
/// in the file; V1.5 omits it there but specifies 1.08 for its benchmark and
/// client inference paths.
fn load_repetition_penalty(model_dir: &Path, default: f64) -> f64 {
    let path = model_dir.join("generation_config.json");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return default;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return default;
    };
    v.get("repetition_penalty")
        .and_then(|x| x.as_f64())
        .unwrap_or(default)
}

/// Read `generation_config.json::eos_token_id`. The official config provides
/// a list (e.g. `[120007, 120020]`). Returns `None` if the file is missing
/// or the field is absent.
fn load_generation_eos_ids(model_dir: &Path) -> Option<Vec<u32>> {
    let contents = std::fs::read_to_string(model_dir.join("generation_config.json")).ok()?;
    let v = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
    let eos = v.get("eos_token_id")?;
    if let Some(single) = eos.as_u64() {
        u32::try_from(single).ok().map(|id| vec![id])
    } else {
        eos.as_array().map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_u64().and_then(|v| u32::try_from(v).ok()))
                .collect()
        })
    }
}

/// Apply HuggingFace's `RepetitionPenaltyLogitsProcessor` rule to a 1D logits
/// tensor and return the argmax id. For each token id that appears in
/// `seen`, the rule pushes its logit toward zero **once**:
/// `logit /= penalty` when positive, `logit *= penalty` when non-positive
/// (see `transformers.generation.logits_process.RepetitionPenaltyLogitsProcessor`).
/// HF computes this with `scatter(input_ids, …)`, which collapses duplicate
/// positions in `input_ids` down to a single penalty per unique vocab id —
/// applying the penalty per *occurrence* would compound to `penalty^k` for a
/// token repeated `k` times and quickly suppresses legitimate high-frequency
/// tokens like `<td>` in a structured HTML page. We dedup before applying.
fn argmax_with_repetition_penalty_cpu(
    logits: &Tensor,
    seen: &[u32],
    penalty: f32,
) -> Result<u32, Error> {
    let mut vec = logits
        .to_dtype(DType::F32)
        .and_then(|t| t.to_vec1::<f32>())
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "rep penalty to_vec1", e))?;
    let vocab = vec.len();
    let mut unique: Vec<u32> = seen.to_vec();
    unique.sort_unstable();
    unique.dedup();
    for &id in &unique {
        let idx = id as usize;
        if idx >= vocab {
            continue;
        }
        let v = vec[idx];
        vec[idx] = if v > 0.0 { v / penalty } else { v * penalty };
    }
    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (i, &v) in vec.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best_idx = i;
        }
    }
    Ok(best_idx as u32)
}

/// Incrementally maintained unique generated-token history. Repetition
/// penalty is presence-based, so keeping another full ordered copy and
/// sorting/deduplicating it for every decode step is unnecessary.
struct RepetitionHistory {
    present: Vec<bool>,
    unique: Vec<u32>,
}

impl RepetitionHistory {
    fn new(vocab_size: usize) -> Self {
        Self {
            present: vec![false; vocab_size],
            unique: Vec::new(),
        }
    }

    fn insert(&mut self, token: u32) {
        let index = token as usize;
        if index < self.present.len() && !self.present[index] {
            self.present[index] = true;
            self.unique.push(token);
        }
    }

    fn contains(&self, token: u32) -> bool {
        self.present.get(token as usize).copied().unwrap_or(false)
    }

    fn ids(&self) -> &[u32] {
        &self.unique
    }

    fn is_empty(&self) -> bool {
        self.unique.is_empty()
    }
}

/// Apply a common unique history to every row plus a small unique per-row
/// suffix. DFlash uses the generated history as the common set and only the
/// preceding proposal tokens as row suffixes, avoiding 16 copies of the full
/// history on every verification round.
fn batched_argmax_with_unique_repetition_parts(
    logits: &Tensor,
    common: &[u32],
    row_extras: &[&[u32]],
    penalty: f32,
) -> Result<Vec<u32>, Error> {
    let vocab_size = logits
        .dim(D::Minus1)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "repetition penalty vocab", e))?;
    let rows = logits.elem_count() / vocab_size;
    if row_extras.len() != rows {
        return Err(Error::Config {
            message: format!(
                "HunyuanOCR: repetition penalty got {} suffix rows for {rows} logits rows",
                row_extras.len()
            ),
        });
    }
    let logits = logits
        .reshape((rows, vocab_size))
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "reshape penalty logits", e))?;

    #[cfg(feature = "cuda")]
    if logits.device().is_cuda() {
        let adjusted = logits
            .to_dtype(DType::F32)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "cast repetition logits", e))?;
        let common: Vec<u32> = common
            .iter()
            .copied()
            .filter(|&token| (token as usize) < vocab_size)
            .collect();
        if !common.is_empty() {
            let common_len = common.len();
            let token_ids =
                Tensor::from_vec(common, (1, common_len), logits.device()).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "upload common repetition ids", e)
                })?;
            adjusted
                .inplace_op2(&token_ids, &RepetitionPenaltyF32 { penalty })
                .map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "GPU common repetition penalty", e)
                })?;
        }

        let token_stride = row_extras
            .iter()
            .map(|extra| extra.len())
            .max()
            .unwrap_or(0);
        if token_stride > 0 {
            let mut token_ids = vec![u32::MAX; rows * token_stride];
            for (row, extra) in row_extras.iter().enumerate() {
                let start = row * token_stride;
                let mut write = start;
                for &token in *extra {
                    if (token as usize) < vocab_size {
                        token_ids[write] = token;
                        write += 1;
                    }
                }
            }
            let token_ids = Tensor::from_vec(token_ids, (rows, token_stride), logits.device())
                .map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "upload row repetition ids", e)
                })?;
            adjusted
                .inplace_op2(&token_ids, &RepetitionPenaltyF32 { penalty })
                .map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "GPU row repetition penalty", e)
                })?;
        }
        return adjusted
            .apply_op1_no_bwd(&ArgmaxFirstF32)
            .and_then(|tokens| tokens.to_vec1::<u32>())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "GPU penalized argmax", e));
    }

    let mut tokens = Vec::with_capacity(rows);
    for (row, extra) in row_extras.iter().enumerate() {
        let row_logits = logits
            .i((row, ..))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "penalty logits row", e))?;
        let mut seen = Vec::with_capacity(common.len() + extra.len());
        seen.extend_from_slice(common);
        seen.extend_from_slice(extra);
        tokens.push(argmax_with_repetition_penalty_cpu(
            &row_logits,
            &seen,
            penalty,
        )?);
    }
    Ok(tokens)
}

fn dflash_argmax_with_host_proposals(
    logits: &Tensor,
    repetition_history: &RepetitionHistory,
    proposals: &[u32],
    penalty: f32,
) -> Result<Vec<u32>, Error> {
    let mut row_extras = Vec::with_capacity(proposals.len() + 1);
    let mut proposal_prefix = Vec::with_capacity(proposals.len());
    for prefix_len in 0..=proposals.len() {
        row_extras.push(proposal_prefix.clone());
        if let Some(&proposal) = proposals.get(prefix_len)
            && !repetition_history.contains(proposal)
            && !proposal_prefix.contains(&proposal)
        {
            proposal_prefix.push(proposal);
        }
    }
    let extra_refs: Vec<&[u32]> = row_extras.iter().map(Vec::as_slice).collect();
    batched_argmax_with_unique_repetition_parts(
        logits,
        repetition_history.ids(),
        &extra_refs,
        penalty,
    )
}

#[cfg(feature = "cuda")]
fn dflash_argmax_with_repetition_penalty(
    logits: &Tensor,
    history: &Tensor,
    proposals: &Tensor,
    penalty: f32,
) -> Result<Vec<u32>, Error> {
    let vocab_size = logits
        .dim(D::Minus1)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "penalty vocab", e))?;
    let rows = logits.elem_count() / vocab_size;
    let proposal_count = proposals
        .dims1()
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "proposal count", e))?;
    if rows != proposal_count + 1 {
        return Err(Error::Config {
            message: format!(
                "HunyuanOCR DFlash: {rows} target rows do not match {proposal_count} proposals"
            ),
        });
    }
    logits
        .reshape((rows, vocab_size))
        .and_then(|logits| {
            logits.apply_op3_no_bwd(history, proposals, &DFlashRepetitionArgmaxBf16 { penalty })
        })
        .and_then(|tokens| tokens.to_vec1::<u32>())
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "penalized argmax", e))
}

#[cfg(feature = "cuda")]
fn mark_repetition_history(history: &Tensor, token_ids: &[u32]) -> Result<(), Error> {
    if token_ids.is_empty() {
        return Ok(());
    }
    let token_ids = Tensor::new(token_ids, history.device())
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "upload repetition history ids", e))?;
    history
        .inplace_op2(&token_ids, &MarkRepetitionHistoryU8)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mark repetition history", e))
}

#[cfg(feature = "cuda")]
fn argmax_with_device_repetition_history(
    logits: &Tensor,
    history: &Tensor,
    penalty: f32,
) -> Result<u32, Error> {
    let vocab_size = logits
        .dim(D::Minus1)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "device penalty vocab", e))?;
    logits
        .reshape((1, vocab_size))
        .and_then(|logits| logits.apply_op2_no_bwd(history, &RepetitionArgmaxBf16 { penalty }))
        .and_then(|tokens| tokens.i(0))
        .and_then(|tokens| tokens.to_scalar::<u32>())
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "device penalized argmax", e))
}

/// Apply repetition penalty to one or more rows and return one greedy token per
/// row. CUDA keeps both the sparse penalty update and argmax on-device; only
/// the resulting token ids cross back to the host. Other backends retain the
/// reference CPU implementation above.
#[cfg(test)]
fn batched_argmax_with_repetition_penalty(
    logits: &Tensor,
    seen_rows: &[&[u32]],
    penalty: f32,
) -> Result<Vec<u32>, Error> {
    let vocab_size = logits
        .dim(D::Minus1)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "repetition penalty vocab", e))?;
    let rows = logits.elem_count() / vocab_size;
    if seen_rows.len() != rows {
        return Err(Error::Config {
            message: format!(
                "HunyuanOCR: repetition penalty got {} history rows for {rows} logits rows",
                seen_rows.len()
            ),
        });
    }
    let mut unique_rows = Vec::with_capacity(rows);
    for seen in seen_rows {
        let mut unique: Vec<u32> = seen
            .iter()
            .copied()
            .filter(|&token| (token as usize) < vocab_size)
            .collect();
        unique.sort_unstable();
        unique.dedup();
        unique_rows.push(unique);
    }
    let unique_refs: Vec<&[u32]> = unique_rows.iter().map(Vec::as_slice).collect();
    batched_argmax_with_unique_repetition_parts(logits, &[], &unique_refs, penalty)
}

fn argmax_with_unique_repetition_penalty(
    logits: &Tensor,
    seen: &[u32],
    penalty: f32,
) -> Result<u32, Error> {
    batched_argmax_with_unique_repetition_parts(logits, seen, &[&[]], penalty)
        .map(|tokens| tokens[0])
}

pub struct HunyuanOcr {
    device: Device,
    dtype: DType,
    cfg: HunyuanOcrConfig,
    image_cfg: HunyuanOcrImageProcessorConfig,
    tokenizer: Tokenizer,
    llm: HunyuanLlm,
    dflash: Option<DFlashModel>,
    vision: HunyuanVisionModel,
    stop_token_ids: Vec<u32>,
    /// `generation_config.json::repetition_penalty`. HuggingFace's
    /// `generate(do_sample=False)` still applies repetition_penalty via the
    /// LogitsProcessor list before the argmax. Without it, large-context chart
    /// inputs can collapse into runaway-repeat loops (e.g. Mermaid node IDs
    /// `A, B, … BZ, BZW, BZWW, BZWWZ …`) that never hit EOS. Default 1.0 means
    /// the value isn't applied.
    repetition_penalty: f64,
    version: HunyuanOcrVersion,
}

impl HunyuanOcr {
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();

        let cfg = HunyuanOcrConfig::from_path(model_dir.join("config.json"))?;
        let version = cfg.version();
        let image_cfg =
            HunyuanOcrImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;

        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("failed to load HunyuanOCR tokenizer.json: {e}"),
            })?;

        let mut stop_token_ids = Vec::new();
        stop_token_ids.push(cfg.eod_token_id);
        stop_token_ids.push(cfg.eos_token_id);
        if let Some(id) = tokenizer.token_to_id("<｜hy_Assistant｜>") {
            stop_token_ids.push(id);
        }
        // Also include eos_token_ids from generation_config.json — the official
        // config lists [120007, 120020]; missing 120007 can cause the model to
        // overshoot past a valid stop point.
        if let Some(gen_eos) = load_generation_eos_ids(model_dir) {
            stop_token_ids.extend(gen_eos);
        }
        stop_token_ids.sort_unstable();
        stop_token_ids.dedup();

        let dtype = crate::utils::select_dtype(&device);

        let weight_files = crate::utils::collect_safetensors(model_dir, "HunyuanOCR")?;
        // SAFETY: from_mmaped_safetensors is unsafe because it memory-maps weight files
        // directly. The caller must ensure the safetensors files are valid and not corrupted.
        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load safetensors shards", e))?
        };

        let llm = HunyuanLlm::load(&cfg, vb.pp("model"))?;
        let vision = HunyuanVisionModel::load(&cfg.vision_config, version, vb.pp("vit"))?;

        // V1.5's generation_config.json omits the value, while its reference
        // benchmark/client paths use 1.08. V1.0 ships its own value (1.03).
        let default_repetition_penalty = match version {
            HunyuanOcrVersion::V1 => 1.0,
            HunyuanOcrVersion::V1_5 => 1.08,
        };
        let repetition_penalty = load_repetition_penalty(model_dir, default_repetition_penalty);

        Ok(Self {
            device,
            dtype,
            cfg,
            image_cfg,
            tokenizer,
            llm,
            dflash: None,
            vision,
            stop_token_ids,
            repetition_penalty,
            version,
        })
    }

    /// Expanded image-token count used to form padding-free parser batches.
    pub(crate) fn recognition_batch_key(&self, image: &RgbImage) -> Option<u64> {
        let (height, width) = super::processing::resized_dimensions(
            image.height(),
            image.width(),
            &self.image_cfg,
            &self.cfg.vision_config,
            self.version,
        )
        .ok()?;
        let factor = (self.image_cfg.patch_size * self.image_cfg.merge_size) as u32;
        let merged_h = height.checked_div(factor)? as usize;
        let merged_w = width.checked_div(factor)? as usize;
        merged_h
            .checked_mul(merged_w.checked_add(1)?)?
            .checked_add(2)
            .map(|tokens| tokens as u64)
    }

    /// Load HunyuanOCR together with an explicitly located DFlash draft.
    ///
    /// DFlash is supported by HunyuanOCR 1.5 only. The official checkpoint
    /// places the draft under `<model_dir>/dflash`.
    pub fn from_dirs(
        model_dir: impl AsRef<Path>,
        dflash_dir: impl AsRef<Path>,
        device: Device,
    ) -> Result<Self, Error> {
        let mut model = Self::from_dir(model_dir, device)?;
        if model.version != HunyuanOcrVersion::V1_5 {
            return Err(Error::Config {
                message: "HunyuanOCR: DFlash requires the 1.5 checkpoint".to_string(),
            });
        }
        let dflash = DFlashModel::from_dir(
            dflash_dir,
            model.dtype,
            &model.device,
            &model.llm.token_embedding_weight(),
        )?;
        if dflash.config().hidden_size != model.cfg.hidden_size
            || dflash.config().vocab_size != model.cfg.vocab_size
        {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: DFlash target mismatch (hidden/vocab draft={}/{}, target={}/{})",
                    dflash.config().hidden_size,
                    dflash.config().vocab_size,
                    model.cfg.hidden_size,
                    model.cfg.vocab_size
                ),
            });
        }
        if dflash
            .config()
            .dflash_config
            .target_layer_ids
            .iter()
            .any(|&id| id >= model.cfg.num_hidden_layers)
        {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: invalid DFlash target layers {:?} for {}-layer target",
                    dflash.config().dflash_config.target_layer_ids,
                    model.cfg.num_hidden_layers
                ),
            });
        }
        let target_layers: Vec<usize> = dflash
            .config()
            .dflash_config
            .target_layer_ids
            .iter()
            .map(|id| id + 1)
            .collect();
        model
            .llm
            .prepare_dflash_cuda_graphs(dflash.config().block_size, &target_layers)?;
        model.dflash = Some(dflash);
        Ok(model)
    }

    /// Load the official `<model_dir>/dflash` draft alongside the target.
    pub fn from_dir_with_dflash(
        model_dir: impl AsRef<Path>,
        device: Device,
    ) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        Self::from_dirs(model_dir, model_dir.join("dflash"), device)
    }

    pub fn dflash_enabled(&self) -> bool {
        self.dflash.is_some()
    }

    /// Override the checkpoint/reference repetition penalty used by greedy
    /// decoding. A value of `1.0` disables the processor, matching the
    /// official DFlash speed-benchmark configuration.
    pub fn set_repetition_penalty(&mut self, penalty: f64) -> Result<(), Error> {
        if !penalty.is_finite() || penalty < 1.0 {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: repetition penalty must be finite and >= 1.0, got {penalty}"
                ),
            });
        }
        self.repetition_penalty = penalty;
        Ok(())
    }

    pub fn repetition_penalty(&self) -> f64 {
        self.repetition_penalty
    }

    /// Number of parallel draft tokens per DFlash step (15 for the official
    /// 16-position bonus+mask block).
    pub fn dflash_num_speculative_tokens(&self) -> Option<usize> {
        self.dflash
            .as_ref()
            .map(|draft| draft.config().block_size.saturating_sub(1))
    }

    /// The checkpoint generation detected from `config.json`.
    pub fn version(&self) -> HunyuanOcrVersion {
        self.version
    }

    /// Generate OCR output for one or more images with custom instructions.
    ///
    /// Supports true GPU batching when multiple images are provided.
    ///
    /// # Arguments
    /// * `images` - Input images
    /// * `instructions` - Instruction for each image (must match images length)
    /// * `max_new_tokens` - Maximum tokens to generate per image
    ///
    /// # Returns
    /// Vector of results, one per input image.
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
                    "HunyuanOCR: images count ({}) != instructions count ({})",
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
    /// excluding stop tokens, before tokenizer decoding or trimming.
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
                    "HunyuanOCR: images count ({}) != instructions count ({})",
                    images.len(),
                    instructions.len()
                ),
            });
        }
        self.generate_tokens_internal(images, instructions, max_new_tokens)
    }

    /// Internal generation implementation supporting batched inference.
    fn generate_tokens_internal(
        &self,
        images: &[RgbImage],
        instructions: &[impl AsRef<str>],
        max_new_tokens: usize,
    ) -> Result<Vec<Vec<u32>>, Error> {
        let batch_size = images.len();

        // 1. Preprocess all images and build prompts
        let mut all_input_ids: Vec<Vec<u32>> = Vec::with_capacity(batch_size);
        let mut all_image_inputs: Vec<HunyuanOcrImageInputs> = Vec::with_capacity(batch_size);

        for (image, instruction) in images.iter().zip(instructions.iter()) {
            let instruction = instruction.as_ref();
            let image_inputs = preprocess_image(
                image,
                &self.image_cfg,
                &self.cfg.vision_config,
                self.version,
                &self.device,
                self.dtype,
            )?;

            let prompt = build_prompt(instruction, self.version);
            let enc = self
                .tokenizer
                .encode(prompt, false)
                .map_err(|e| Error::InvalidInput {
                    message: format!("HunyuanOCR: tokenizer encode failed: {e}"),
                })?;

            let mut input_ids = enc.get_ids().to_vec();
            expand_image_tokens_in_place(&mut input_ids, &self.cfg, &image_inputs)?;

            all_input_ids.push(input_ids);
            all_image_inputs.push(image_inputs);
        }

        // 2. Compute vision features and build embeddings for each sample
        let seq_lens: Vec<usize> = all_input_ids.iter().map(|ids| ids.len()).collect();
        let max_seq_len = *seq_lens.iter().max().unwrap();

        let mut batch_embeds: Vec<Tensor> = Vec::with_capacity(batch_size);
        let mut batch_position_ids: Vec<Tensor> = Vec::with_capacity(batch_size);

        for (input_ids, image_inputs) in all_input_ids.iter().zip(all_image_inputs.iter()) {
            let seq_len = input_ids.len();
            let pad_len = max_seq_len - seq_len;

            // Embed tokens
            let input_ids_t = Tensor::new(input_ids.clone(), &self.device)
                .and_then(|t| t.reshape((1, seq_len)))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create input_ids", e))?;
            let token_embeds = self.llm.embed(&input_ids_t)?;

            // Get vision features
            let (image_embeds, merged_hw) = self.vision.forward(&image_inputs.pixel_values)?;
            if merged_hw
                != (
                    image_inputs.grid_thw_merged.1,
                    image_inputs.grid_thw_merged.2,
                )
            {
                return Err(Error::InvalidInput {
                    message: format!(
                        "HunyuanOCR: merged grid mismatch: vision={:?} preprocessor={:?}",
                        merged_hw, image_inputs.grid_thw_merged
                    ),
                });
            }

            // Fuse image embeddings into the token embedding sequence.
            let (start_pos, end_pos) = find_image_span(input_ids, &self.cfg)?;
            let inner_len = end_pos.saturating_sub(start_pos + 1);
            let (img_len, _) = image_embeds
                .dims2()
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "image_embeds dims2", e))?;
            if inner_len != img_len {
                return Err(Error::InvalidInput {
                    message: format!(
                        "HunyuanOCR: image-token run length mismatch: tokens={inner_len} embeds={img_len}"
                    ),
                });
            }

            let token_embeds = token_embeds.squeeze(0).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "squeeze token embeddings", e)
            })?;

            let mut parts: Vec<Tensor> = Vec::with_capacity(3);
            // Prefix incl. image_start (text-embedded).
            parts.push(token_embeds.i((0..=start_pos, ..)).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "slice prefix embeddings", e)
            })?);
            parts.push(image_embeds);
            if end_pos < input_ids.len() {
                // Suffix incl. image_end (text-embedded).
                parts.push(
                    token_embeds
                        .i((end_pos..input_ids.len(), ..))
                        .map_err(|e| {
                            candle_to_ocr_inference("HunyuanOCR", "slice suffix embeddings", e)
                        })?,
                );
            }
            let refs: Vec<&Tensor> = parts.iter().collect();
            let mut inputs_embeds = Tensor::cat(&refs, 0)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "cat embeds", e))?
                .unsqueeze(0)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "unsqueeze embeds", e))?;

            // Left-pad if needed
            if pad_len > 0 {
                let hidden_size = inputs_embeds
                    .dim(2)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "get hidden_size", e))?;
                let pad = Tensor::zeros(
                    (1, pad_len, hidden_size),
                    inputs_embeds.dtype(),
                    &self.device,
                )
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create pad", e))?;
                inputs_embeds = Tensor::cat(&[&pad, &inputs_embeds], 1)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "cat pad", e))?;
            }
            batch_embeds.push(inputs_embeds);

            // Build position IDs
            let pos_ids = build_position_ids(input_ids, &self.cfg, image_inputs)?;
            // Left-pad position IDs
            let pos_ids = if pad_len > 0 {
                let pad_pos = Tensor::zeros((4, 1, pad_len), DType::I64, &self.device)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create pad pos", e))?;
                Tensor::cat(&[&pad_pos, &pos_ids], 2)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "cat pad pos", e))?
            } else {
                pos_ids
            };
            batch_position_ids.push(pos_ids);
        }

        // 3. Stack batched tensors
        let batch_refs: Vec<&Tensor> = batch_embeds.iter().collect();
        let inputs_embeds = Tensor::cat(&batch_refs, 0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "stack embeds", e))?;

        let pos_refs: Vec<&Tensor> = batch_position_ids.iter().collect();
        let position_ids = Tensor::cat(&pos_refs, 1)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "stack pos", e))?;

        // 4. Create attention mask
        let mask = if batch_size == 1 && self.device.is_cuda() {
            None
        } else {
            let causal = create_causal_mask(max_seq_len, max_seq_len, self.dtype, &self.device)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create causal", e))?;
            let padding =
                create_left_padding_mask(&seq_lens, max_seq_len, self.dtype, &self.device)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create padding", e))?;
            Some(
                combine_masks(&causal, &padding)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "combine masks", e))?,
            )
        };

        // 5. Prefill
        self.llm.clear_kv_cache();
        let active_dflash = if batch_size == 1 {
            if let Some(dflash) = self.dflash.as_ref() {
                // A previous multi-image request or an over-capacity decode
                // may have invalidated either graph. Recreate them before
                // prefill; steady state is an inexpensive capacity check.
                dflash.prepare_cuda_graph()?;
                let target_layers: Vec<usize> = dflash
                    .config()
                    .dflash_config
                    .target_layer_ids
                    .iter()
                    .map(|id| id + 1)
                    .collect();
                self.llm
                    .prepare_dflash_cuda_graphs(dflash.config().block_size, &target_layers)?;
            } else {
                self.llm
                    .prepare_ar_cuda_graph(max_seq_len, max_new_tokens)?;
            }
            self.dflash.as_ref()
        } else {
            // Multi-image requests disable DFlash and prefill a batch>1
            // shape, which is incompatible with the batch=1 KV storage a
            // captured target CUDA graph points at. Drop the graph before
            // that reallocation so a later single-image DFlash decode can't
            // replay it against freed memory.
            self.llm.invalidate_target_cuda_graph();
            None
        };
        let hidden = if let Some(dflash) = active_dflash {
            // DFlash stores zero-based target layer indices. HunyuanLlm uses
            // one-based post-layer boundaries, matching vLLM's `i + 1`
            // conversion before auxiliary-state capture.
            let aux_layer_ids: Vec<usize> = dflash
                .config()
                .dflash_config
                .target_layer_ids
                .iter()
                .map(|id| id + 1)
                .collect();
            let output = self.llm.forward_with_aux(
                &inputs_embeds,
                &position_ids,
                mask.as_ref(),
                &aux_layer_ids,
            )?;
            let aux = output.aux_hidden_states.ok_or_else(|| Error::Config {
                message: "HunyuanOCR DFlash: target did not return auxiliary hidden states"
                    .to_string(),
            })?;
            dflash.reset_context(&aux)?;
            output.hidden_states
        } else {
            self.llm
                .forward(&inputs_embeds, &position_ids, mask.as_ref())?
        };

        // 6. Get initial logits per sample
        let mut logits_list: Vec<Tensor> = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            let last = hidden
                .i((i, max_seq_len - 1, ..))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "get last hidden", e))?;
            let logits = self.logits_from_hidden(&last)?;
            logits_list.push(logits);
        }

        // DFlash currently targets the latency-sensitive single-document path.
        // Multi-image calls retain the existing true-batch autoregressive path.
        if let Some(dflash) = active_dflash {
            let initial_logits = logits_list.pop().ok_or_else(|| Error::Config {
                message: "HunyuanOCR DFlash: missing target prefill logits".to_string(),
            })?;
            let tokens =
                self.generate_dflash_tokens(dflash, initial_logits, seq_lens[0], max_new_tokens)?;
            return Ok(vec![tokens]);
        }

        // 7. Autoregressive decode
        let mut generated: Vec<Vec<u32>> = vec![Vec::new(); batch_size];
        let mut repetition_histories: Vec<RepetitionHistory> = (0..batch_size)
            .map(|_| RepetitionHistory::new(self.cfg.vocab_size))
            .collect();
        #[cfg(feature = "cuda")]
        let repetition_history_device = if batch_size == 1
            && self.device.is_cuda()
            && self.dtype == DType::BF16
            && self.repetition_penalty > 1.0
        {
            Some(
                Tensor::zeros((1, self.cfg.vocab_size), DType::U8, &self.device).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "allocate AR repetition history", e)
                })?,
            )
        } else {
            None
        };
        let mut finished: Vec<bool> = vec![false; batch_size];
        let mut positions: Vec<i64> = seq_lens.iter().map(|&len| len as i64).collect();

        // Left-padding lengths per row, and current KV-cache length (grows by one
        // each decode step). Used to mask out padding KV during generation so a
        // batch with unequal prompt lengths does not attend to padding positions.
        let pad_lens: Vec<usize> = seq_lens.iter().map(|&len| max_seq_len - len).collect();
        let mut kv_len = max_seq_len;

        for _ in 0..max_new_tokens {
            if finished.iter().all(|&f| f) {
                break;
            }

            let mut next_tokens: Vec<u32> = Vec::with_capacity(batch_size);
            for (i, logits) in logits_list.iter().enumerate() {
                if finished[i] {
                    next_tokens.push(0); // Padding token for finished samples
                } else {
                    // Mirror HuggingFace's `generate(do_sample=False)`: even
                    // greedy decoding runs the LogitsProcessor list, so the
                    // `repetition_penalty` from generation_config.json gets
                    // applied to logits before argmax. Without this the model
                    // can spiral into runaway-repeat loops on large-context
                    // inputs (observed on chart_01.jpg, seq≈11584, producing
                    // 33K chars of synthetic Mermaid node IDs `BZ, BZW, …`).
                    let tok =
                        if self.repetition_penalty > 1.0 && !repetition_histories[i].is_empty() {
                            #[cfg(feature = "cuda")]
                            {
                                if let Some(history) = repetition_history_device.as_ref() {
                                    argmax_with_device_repetition_history(
                                        logits,
                                        history,
                                        self.repetition_penalty as f32,
                                    )?
                                } else {
                                    argmax_with_unique_repetition_penalty(
                                        logits,
                                        repetition_histories[i].ids(),
                                        self.repetition_penalty as f32,
                                    )?
                                }
                            }
                            #[cfg(not(feature = "cuda"))]
                            {
                                argmax_with_unique_repetition_penalty(
                                    logits,
                                    repetition_histories[i].ids(),
                                    self.repetition_penalty as f32,
                                )?
                            }
                        } else {
                            logits
                                .argmax(D::Minus1)
                                .and_then(|t| t.to_scalar::<u32>())
                                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "argmax", e))?
                        };

                    if self.stop_token_ids.contains(&tok) {
                        finished[i] = true;
                    } else {
                        generated[i].push(tok);
                        repetition_histories[i].insert(tok);
                        #[cfg(feature = "cuda")]
                        if i == 0
                            && let Some(history) = repetition_history_device.as_ref()
                        {
                            mark_repetition_history(history, &[tok])?;
                        }
                    }
                    next_tokens.push(tok);
                }
            }

            if finished.iter().all(|&f| f) {
                break;
            }

            // Batch forward for next tokens
            let tokens = Tensor::new(next_tokens, &self.device)
                .and_then(|t| t.reshape((batch_size, 1)))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create tokens", e))?;
            let embeds = self.llm.embed(&tokens)?;

            kv_len += 1;
            let (hs, graph_logits) = if batch_size == 1 {
                // There is no left padding and a one-token query cannot see a
                // future key. Reuse the precomputed scalar-position RoPE cache
                // and avoid rebuilding four-axis positions plus an unused
                // generation mask on every decode step.
                let output =
                    self.llm
                        .forward_with_aux_decode(&embeds, positions[0] as usize, None, &[])?;
                (output.hidden_states, output.logits)
            } else {
                // Build 4-axis position IDs for the true-batch decode path.
                let pos_data = crate::attention::decode_position_buffer(&positions, 4);
                let pos = Tensor::new(pos_data, &self.device)
                    .and_then(|t| t.reshape((4, batch_size, 1)))
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create pos", e))?;

                // Mask out left-padding positions in the KV cache for this
                // step when prompt lengths differ across the batch.
                let gen_mask =
                    create_generation_mask_if_needed(&pad_lens, kv_len, self.dtype, &self.device)
                        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "create gen mask", e))?;
                (self.llm.forward(&embeds, &pos, gen_mask.as_ref())?, None)
            };

            logits_list.clear();
            if let Some(logits) = graph_logits {
                let logits = logits
                    .i((0, 0, ..))
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "get graph logits", e))?;
                logits_list.push(logits);
            } else {
                for i in 0..batch_size {
                    let h = hs
                        .i((i, 0, ..))
                        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "get hs", e))?;
                    let logits = self.logits_from_hidden(&h)?;
                    logits_list.push(logits);
                }
            }

            for (i, p) in positions.iter_mut().enumerate() {
                if !finished[i] {
                    *p += 1;
                }
            }
        }

        self.llm.clear_kv_cache();
        Ok(generated)
    }

    /// Greedy DFlash speculative decoding for one document. The target model
    /// remains authoritative: accepted draft prefixes plus the first target
    /// recovery/bonus token are exactly equivalent to autoregressive greedy
    /// decoding, including repetition-penalty processing.
    fn generate_dflash_tokens(
        &self,
        dflash: &DFlashModel,
        initial_logits: Tensor,
        prompt_len: usize,
        max_new_tokens: usize,
    ) -> Result<Vec<u32>, Error> {
        if max_new_tokens == 0 {
            self.llm.clear_kv_cache();
            dflash.clear_context();
            return Ok(Vec::new());
        }

        let first = initial_logits
            .argmax(D::Minus1)
            .and_then(|token| token.to_scalar::<u32>())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "initial argmax", e))?;
        if self.stop_token_ids.contains(&first) {
            self.llm.clear_kv_cache();
            dflash.clear_context();
            return Ok(Vec::new());
        }

        let num_spec = dflash.config().block_size.saturating_sub(1);
        if num_spec == 0 {
            return Err(Error::Config {
                message: "HunyuanOCR DFlash: block_size must include at least one mask position"
                    .to_string(),
            });
        }
        let mask_id = dflash.config().dflash_config.mask_token_id;
        let target_layers: Vec<usize> = dflash
            .config()
            .dflash_config
            .target_layer_ids
            .iter()
            .map(|id| id + 1)
            .collect();
        let mut generated = vec![first];
        let mut repetition_history = RepetitionHistory::new(self.cfg.vocab_size);
        repetition_history.insert(first);
        #[cfg(feature = "cuda")]
        let repetition_history_device = if self.device.is_cuda()
            && self.dtype == DType::BF16
            && self.repetition_penalty > 1.0
        {
            let history =
                Tensor::zeros((self.cfg.vocab_size,), DType::U8, &self.device).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR DFlash", "allocate repetition history", e)
                })?;
            mark_repetition_history(&history, &[first])?;
            Some(history)
        } else {
            None
        };
        let mut draft_rounds = 0usize;
        let mut accepted_draft_tokens = 0usize;

        while generated.len() < max_new_tokens {
            draft_rounds += 1;
            let bonus = *generated
                .last()
                .expect("generated contains the bonus token");

            // One bonus query followed by parallel mask queries.
            let mut query_ids = Vec::with_capacity(num_spec + 1);
            query_ids.push(bonus);
            query_ids.resize(num_spec + 1, mask_id);
            let query_ids = Tensor::new(query_ids, &self.device)
                .and_then(|t| t.reshape((1, num_spec + 1)))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "query ids", e))?;
            let query_embeds = self.llm.embed(&query_ids)?;
            // All draft rows are independent. The CUDA graph includes their
            // shared LM head and argmax; target-side processors remain
            // authoritative during verification.
            let proposals = dflash.forward_proposals(&query_embeds)?;

            // The target verifies [bonus, proposal_1, ..., proposal_N] in one
            // causal pass. Row i predicts proposal i+1; the final row predicts
            // the target-only bonus token used when every proposal is accepted.
            let context_len = self.llm.kv_cache_len();
            debug_assert_eq!(context_len, dflash.context_len());
            // Keep proposals on the device while preparing target verification.
            // Reading them here would serialize the CPU after the draft pass;
            // instead, the target pass is queued immediately and the proposal
            // ids are copied back only after target sampling has synchronized
            // the stream.
            let bonus_id = query_ids
                .narrow(1, 0, 1)
                .and_then(|t| t.squeeze(0))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "bonus id view", e))?;
            let verify_ids = Tensor::cat(&[&bonus_id, &proposals], 0)
                .and_then(|t| t.reshape((1, num_spec + 1)))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR DFlash", "verify ids", e))?;
            let verify_embeds = self.llm.embed(&verify_ids)?;
            let verify_mask = if self.device.is_cuda() {
                None
            } else {
                Some(
                    create_causal_mask(
                        num_spec + 1,
                        context_len + num_spec + 1,
                        self.dtype,
                        &self.device,
                    )
                    .map_err(|e| {
                        candle_to_ocr_inference("HunyuanOCR DFlash", "verify causal mask", e)
                    })?,
                )
            };
            let output = self.llm.forward_with_aux_decode(
                &verify_embeds,
                context_len,
                verify_mask.as_ref(),
                &target_layers,
            )?;
            let aux = output.aux_hidden_states.ok_or_else(|| Error::Config {
                message: "HunyuanOCR DFlash: target verification did not return auxiliary states"
                    .to_string(),
            })?;
            let target_logits = if let Some(logits) = output.logits.as_ref() {
                logits.squeeze(0).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR DFlash", "graph target logits", e)
                })?
            } else {
                let target_rows = output.hidden_states.squeeze(0).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR DFlash", "target verify rows", e)
                })?;
                self.logits_from_hidden_batch(&target_rows)?
            };
            // CPU backends need proposal values to build each row's history.
            // CUDA keeps them device-resident through target sampling so the
            // target-token transfer is the round's only blocking D2H copy.
            let proposals_host = if self.device.is_cuda()
                && (self.repetition_penalty <= 1.0 || self.dtype == DType::BF16)
            {
                None
            } else {
                Some(proposals.to_vec1::<u32>().map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR DFlash", "draft proposals", e)
                })?)
            };
            let target_tokens = if self.repetition_penalty <= 1.0 {
                target_logits
                    .argmax(D::Minus1)
                    .and_then(|t| t.to_vec1::<u32>())
                    .map_err(|e| {
                        candle_to_ocr_inference("HunyuanOCR DFlash", "batched target argmax", e)
                    })?
            } else {
                #[cfg(feature = "cuda")]
                {
                    if self.device.is_cuda() && self.dtype == DType::BF16 {
                        dflash_argmax_with_repetition_penalty(
                            &target_logits,
                            repetition_history_device
                                .as_ref()
                                .ok_or_else(|| Error::Config {
                                    message: "HunyuanOCR DFlash: missing CUDA repetition history"
                                        .to_string(),
                                })?,
                            &proposals,
                            self.repetition_penalty as f32,
                        )?
                    } else {
                        dflash_argmax_with_host_proposals(
                            &target_logits,
                            &repetition_history,
                            proposals_host
                                .as_deref()
                                .expect("non-CUDA proposals were copied to the host"),
                            self.repetition_penalty as f32,
                        )?
                    }
                }
                #[cfg(not(feature = "cuda"))]
                {
                    dflash_argmax_with_host_proposals(
                        &target_logits,
                        &repetition_history,
                        proposals_host
                            .as_deref()
                            .expect("non-CUDA proposals were copied to the host"),
                        self.repetition_penalty as f32,
                    )?
                }
            };
            // Target sampling above has already synchronized CUDA. This small
            // proposal copy is now ready immediately and does not stall the
            // draft/target pipeline a second time.
            let proposals = match proposals_host {
                Some(proposals) => proposals,
                None => proposals.to_vec1::<u32>().map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR DFlash", "draft proposals", e)
                })?,
            };

            let mut accepted = 0usize;
            let mut recovery = None;
            for (index, &proposal) in proposals.iter().enumerate() {
                let target_token = target_tokens[index];
                if target_token != proposal {
                    recovery = Some(target_token);
                    break;
                }
                accepted += 1;
            }
            accepted_draft_tokens += accepted;

            let (processed_inputs, next_tokens) = if let Some(recovery) = recovery {
                let mut tokens = proposals[..accepted].to_vec();
                tokens.push(recovery);
                // bonus plus the accepted proposal prefix were actually part
                // of the authoritative history before the recovery token.
                (1 + accepted, tokens)
            } else {
                let target_bonus = target_tokens[num_spec];
                let mut tokens = proposals;
                tokens.push(target_bonus);
                (num_spec + 1, tokens)
            };
            tracing::debug!(
                accepted,
                processed_inputs,
                proposals = ?next_tokens.get(..accepted),
                recovery_or_bonus = ?next_tokens.last(),
                "HunyuanOCR DFlash verification"
            );

            // Verification appended the whole candidate block. Keep only the
            // prefix that precedes the newly emitted recovery/bonus token.
            self.llm.trim_kv_cache(context_len + processed_inputs)?;
            let accepted_aux = aux.narrow(1, 0, processed_inputs).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR DFlash", "accepted auxiliary states", e)
            })?;
            dflash.append_context(&accepted_aux)?;

            let mut stop = false;
            #[cfg(feature = "cuda")]
            let mut new_history_tokens = Vec::with_capacity(next_tokens.len());
            for token in next_tokens {
                if self.stop_token_ids.contains(&token) {
                    stop = true;
                    break;
                }
                if generated.len() == max_new_tokens {
                    stop = true;
                    break;
                }
                generated.push(token);
                repetition_history.insert(token);
                #[cfg(feature = "cuda")]
                new_history_tokens.push(token);
            }
            if stop {
                break;
            }
            #[cfg(feature = "cuda")]
            if let Some(history) = repetition_history_device.as_ref() {
                mark_repetition_history(history, &new_history_tokens)?;
            }

            debug_assert_eq!(self.llm.kv_cache_len(), dflash.context_len());
            debug_assert!(self.llm.kv_cache_len() >= prompt_len);
        }

        if draft_rounds > 0 {
            tracing::info!(
                rounds = draft_rounds,
                accepted = accepted_draft_tokens,
                drafted = draft_rounds * num_spec,
                acceptance_rate = accepted_draft_tokens as f64 / (draft_rounds * num_spec) as f64,
                mean_acceptance_length = 1.0 + accepted_draft_tokens as f64 / draft_rounds as f64,
                "HunyuanOCR DFlash metrics"
            );
        }

        self.llm.clear_kv_cache();
        dflash.clear_context();
        Ok(generated)
    }

    pub fn decode_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        self.decode_generated_tokens(tokens)
    }

    /// Decode tokens in the form the model actually emitted. HunyuanOCR's
    /// `decode_tokens` only applies `trim()` post-process, so this is
    /// effectively an alias provided for API symmetry with backends that do
    /// have non-trivial post-process (PaddleOCR-VL / GLM-OCR).
    pub fn decode_tokens_raw(&self, tokens: &[u32]) -> Result<String, Error> {
        self.tokenizer
            .decode(tokens, true)
            .map_err(|e| Error::InvalidInput {
                message: format!("HunyuanOCR: tokenizer decode failed: {e}"),
            })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    fn decode_generated_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        Ok(self.decode_tokens_raw(tokens)?.trim().to_string())
    }

    fn logits_from_hidden(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let hidden = hidden.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: unsqueeze hidden failed",
                e,
            )
        })?;
        let w = self.llm.token_embedding_weight();
        let wt = w.transpose(0, 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: transpose embedding weight failed",
                e,
            )
        })?;
        hidden
            .matmul(&wt)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "matmul logits", e))?
            .squeeze(0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "squeeze logits", e))
    }

    fn logits_from_hidden_batch(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let w = self.llm.token_embedding_weight();
        let wt = w.transpose(0, 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: transpose embedding weight failed",
                e,
            )
        })?;
        hidden
            .matmul(&wt)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "batched matmul logits", e))
    }
}

fn build_prompt(instruction: &str, version: HunyuanOcrVersion) -> String {
    // V1.0's reference invocation supplies an empty system message, which the
    // template renders as placeholder no.3. V1.5's official invocation starts
    // directly with the user message and therefore omits that token.
    let system_prefix = match version {
        HunyuanOcrVersion::V1 => "<｜hy_place▁holder▁no▁3｜>",
        HunyuanOcrVersion::V1_5 => "",
    };
    format!(
        "<｜hy_begin▁of▁sentence｜>{system_prefix}\
<｜hy_place▁holder▁no▁100｜><｜hy_place▁holder▁no▁102｜><｜hy_place▁holder▁no▁101｜>{instruction}\
<｜hy_User｜>"
    )
}

fn expand_image_tokens_in_place(
    input_ids: &mut Vec<u32>,
    cfg: &HunyuanOcrConfig,
    image_inputs: &HunyuanOcrImageInputs,
) -> Result<(), Error> {
    let (_, hm, wm) = image_inputs.grid_thw_merged;
    // Upstream processor: num_image_tokens = patch_h * (patch_w + 1) + 2
    // (processing_hunyuan_vl.py:62). The `+ 2` covers the perceive step's
    // begin/end markers, whose positions are also replaced by image
    // embeddings. The placeholder run is contiguous `image_token_id` only —
    // no `image_newline_token_id` interleaving.
    let expected_tokens = hm.saturating_mul(wm.saturating_add(1)).saturating_add(2);
    if expected_tokens == 0 {
        return Err(Error::InvalidInput {
            message: "HunyuanOCR: empty merged grid".to_string(),
        });
    }

    let mut placeholder = None;
    for i in 0..input_ids.len().saturating_sub(2) {
        if input_ids[i] == cfg.image_start_token_id
            && input_ids[i + 1] == cfg.image_token_id
            && input_ids[i + 2] == cfg.image_end_token_id
        {
            placeholder = Some(i + 1);
            break;
        }
    }
    let Some(pos) = placeholder else {
        return Err(Error::InvalidInput {
            message: "HunyuanOCR: prompt is missing image placeholder tokens".to_string(),
        });
    };

    let expanded: Vec<u32> = std::iter::repeat_n(cfg.image_token_id, expected_tokens).collect();
    // Replace the single image_token_id placeholder with the expanded run.
    // image_start_token_id and image_end_token_id stay in input_ids on
    // either side; they receive plain text embeddings via embed_tokens.
    input_ids.splice(pos..pos + 1, expanded);
    Ok(())
}

fn find_image_span(input_ids: &[u32], cfg: &HunyuanOcrConfig) -> Result<(usize, usize), Error> {
    let start_pos = input_ids
        .iter()
        .position(|&id| id == cfg.image_start_token_id)
        .ok_or_else(|| Error::InvalidInput {
            message: "HunyuanOCR: image_start_token_id not found in input_ids".to_string(),
        })?;
    let end_pos = input_ids[start_pos + 1..]
        .iter()
        .position(|&id| id == cfg.image_end_token_id)
        .map(|p| start_pos + 1 + p)
        .ok_or_else(|| Error::InvalidInput {
            message: "HunyuanOCR: image_end_token_id not found after image_start_token_id"
                .to_string(),
        })?;
    Ok((start_pos, end_pos))
}

fn build_position_ids(
    input_ids: &[u32],
    cfg: &HunyuanOcrConfig,
    image_inputs: &HunyuanOcrImageInputs,
) -> Result<Tensor, Error> {
    let seq_len = input_ids.len();
    // 4-axis XDRoPE position ids matching the upstream HF processor exactly:
    //   the upstream Transformers Hunyuan-VL processor, lines 74-94.
    //
    // Axis order is `[seq, w, h, t]` (the order `select_rope_sections`
    // expects for `xdrope_section`). For non-image tokens all four axes hold
    // the plain sequence index. For the spatial run inside the image span we
    // overwrite axes w/h/t:
    //   - w cycles `0..(patch_w+1)`, repeated `patch_h` times,
    //   - h is `[h]*(patch_w+1)` for `h` in `0..patch_h`,
    //   - t is 0 across the run.
    // The run starts at `first_image_token + 1` and spans `(patch_w+1)*patch_h`
    // tokens — the *middle* of the expanded `patch_h*(patch_w+1) + 2` block;
    // the perceive begin/end markers keep their default arange position.
    // Collapsing the spatial axes to plain 1-D sequence ids destroys the 2-D
    // geometry the trained weights expect and yields hallucinated text.
    let mut pos: Vec<i64> = vec![0; 4 * seq_len];
    for i in 0..seq_len {
        let p = i as i64;
        pos[i] = p;
        pos[seq_len + i] = p;
        pos[2 * seq_len + i] = p;
        pos[3 * seq_len + i] = p;
    }

    let first_image_pos = input_ids.iter().position(|&id| id == cfg.image_token_id);
    if let Some(first) = first_image_pos {
        let (_, hm, wm) = image_inputs.grid_thw_merged;
        let start = first + 1;
        let replace_num = (wm + 1) * hm;
        if start + replace_num > seq_len {
            return Err(Error::InvalidInput {
                message: format!(
                    "HunyuanOCR: image span ({} positions starting at {}) exceeds input length {}",
                    replace_num, start, seq_len
                ),
            });
        }
        for j in 0..replace_num {
            let idx = start + j;
            let row = j / (wm + 1); // 0..hm
            let col = j % (wm + 1); // 0..wm (inclusive of newline column)
            pos[seq_len + idx] = col as i64; // axis 1 = w
            pos[2 * seq_len + idx] = row as i64; // axis 2 = h
            pos[3 * seq_len + idx] = 0; // axis 3 = t
        }
    }

    Tensor::from_vec(
        pos,
        (4usize, 1usize, seq_len),
        image_inputs.pixel_values.device(),
    )
    .map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "HunyuanOCR: build position_ids tensor failed",
            e,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_repetition_penalty_tokens(device: &Device) {
        let logits = Tensor::from_vec(
            vec![
                10.0f32, 9.0, 8.0, -1.0, // duplicate + out-of-range ids
                -1.0, -2.0, 3.0, 2.5, // positive penalized winner
                -1.0, -1.1, -1.2, -1.3, // negative penalized winner
            ],
            (3, 4),
            device,
        )
        .unwrap();
        let row0 = [0u32, 0, 99];
        let row1 = [2u32, 2];
        let row2 = [0u32];
        let histories: [&[u32]; 3] = [&row0, &row1, &row2];
        let tokens = batched_argmax_with_repetition_penalty(&logits, &histories, 2.0).unwrap();
        assert_eq!(tokens, vec![1, 3, 1]);
    }

    #[test]
    fn repetition_penalty_cpu_matches_huggingface_semantics() {
        assert_repetition_penalty_tokens(&Device::Cpu);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn repetition_penalty_cuda_matches_cpu() {
        let Ok(device) = Device::new_cuda(0) else {
            return;
        };
        assert_repetition_penalty_tokens(&device);

        // Candle's generic CUDA argmax can choose index 1024 here because the
        // equal maxima live in different reduction lanes. The reference CPU
        // scan, and our deterministic kernel, must keep the first index.
        let mut tied = vec![0.0f32; 1030];
        tied[1] = 10.0;
        tied[1024] = 10.0;
        let logits = Tensor::from_vec(tied, (1, 1030), &device).unwrap();
        let history = [2u32];
        let tokens = batched_argmax_with_repetition_penalty(&logits, &[&history], 1.08).unwrap();
        assert_eq!(tokens, vec![1]);

        let mut tied = vec![0.0f32; 1030];
        tied[1] = 10.0;
        tied[1024] = 10.0;
        let logits = Tensor::from_vec(tied, (1, 1030), &device)
            .unwrap()
            .to_dtype(DType::BF16)
            .unwrap();
        let history = Tensor::zeros((1, 1030), DType::U8, &device).unwrap();
        mark_repetition_history(&history, &[2]).unwrap();
        let token = argmax_with_device_repetition_history(&logits, &history, 1.08).unwrap();
        assert_eq!(token, 1);

        let logits = Tensor::from_vec(
            vec![10.0f32, 9.0, 0.0, 0.0, 8.0, 7.0, 0.0, 0.0],
            (2, 4),
            &device,
        )
        .unwrap();
        let empty: &[u32] = &[];
        let tokens =
            batched_argmax_with_unique_repetition_parts(&logits, &[0], &[empty, empty], 2.0)
                .unwrap();
        assert_eq!(tokens, vec![1, 1]);

        // Row r sees only proposal ids [0, r), with ids already present in
        // the common history and duplicate proposals penalized exactly once.
        let logits = Tensor::from_vec(
            vec![
                10.0f32, 9.0, 5.2, 0.0, // common history only
                10.0, 9.0, 5.2, 0.0, // common + proposal 1
                10.0, 20.0, 7.0, 0.0, // duplicate proposal 1 stays once
                10.0, 20.0, 7.0, 0.0, // proposal 0 is already common
            ],
            (4, 4),
            &device,
        )
        .unwrap()
        .to_dtype(DType::BF16)
        .unwrap();
        let proposals = Tensor::new(&[1u32, 1, 0], &device).unwrap();
        let history = Tensor::zeros((4,), DType::U8, &device).unwrap();
        mark_repetition_history(&history, &[0, 0, 99]).unwrap();
        let tokens =
            dflash_argmax_with_repetition_penalty(&logits, &history, &proposals, 2.0).unwrap();
        assert_eq!(tokens, vec![1, 2, 1, 1]);
    }

    #[test]
    fn v15_prompt_omits_legacy_empty_system_token() {
        let prompt = build_prompt("read", HunyuanOcrVersion::V1_5);
        assert!(prompt.starts_with("<｜hy_begin▁of▁sentence｜><｜hy_place▁holder▁no▁100｜>"));
        assert!(!prompt.contains("<｜hy_place▁holder▁no▁3｜>"));
        assert!(prompt.ends_with("read<｜hy_User｜>"));
    }

    #[test]
    fn v1_prompt_keeps_empty_system_token() {
        let prompt = build_prompt("read", HunyuanOcrVersion::V1);
        assert!(prompt.starts_with("<｜hy_begin▁of▁sentence｜><｜hy_place▁holder▁no▁3｜>"));
    }
}
