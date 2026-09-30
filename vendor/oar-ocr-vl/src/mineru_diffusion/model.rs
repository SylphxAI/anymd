//! MinerU-Diffusion-V1 model: Qwen2-VL vision tower + `patch_merger2x`
//! abstractor + SDAR block-diffusion text decoder.
//!
//! Generation follows the reference `generate_with_embeds`: the prompt is
//! prefilled with a block-causal mask, then the answer is produced
//! block-by-block. Within each block every position starts as `<|MASK|>` and is
//! iteratively *unmasked* — at each denoising step the decoder predicts all
//! still-masked positions, and the highest-confidence ones (those above
//! `dynamic_threshold`, or at least `num_transfer_tokens[step]`) are committed.
//! Once a block is fully decoded its KV is committed to the cache and the next
//! block begins.

use super::config::MinerUDiffusionConfig;
use super::projector::VisionAbstractor;
use super::text::{SdarKvCache, SdarModel};
#[cfg(feature = "cuda")]
use crate::cuda_kernels::SampleWithConfidence;
use crate::doc_parser::{RecognitionBackend, RecognitionTask};
use crate::error::Error;
use crate::mineru::MinerUImageProcessorConfig;
use crate::mineru::processing::preprocess_images;
use crate::mineru::vision::MinerUVisionModel;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, Device, Tensor};
use image::RgbImage;
use rand::SeedableRng;
use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;
use rand::rngs::StdRng;
use std::path::Path;
use tokenizers::Tokenizer;

/// Default per-task instruction for full-page text recognition.
pub const DEFAULT_PROMPT: &str = "\nText Recognition:";

/// Tunable knobs for one diffusion decode.
///
/// Defaults match the reference `generate_with_embeds` signature and the
/// README inference call: `temperature = 1.0`, `top_k = 0`, `top_p = 1.0`,
/// `dynamic_threshold = 0.95`. The reference samples each denoising step with
/// `torch.multinomial` (see `sample_tokens_and_conf`); `seed` makes that
/// sampling reproducible. Set `temperature <= 0.0` for deterministic greedy
/// (argmax) decoding, which ignores `seed`/`top_k`/`top_p`.
#[derive(Debug, Clone)]
pub struct DiffusionGenerationConfig {
    pub gen_length: usize,
    pub block_length: usize,
    pub denoising_steps: usize,
    pub dynamic_threshold: f32,
    /// Sampling temperature. `1.0` matches the reference; `<= 0.0` selects
    /// deterministic greedy decoding.
    pub temperature: f32,
    /// Top-k logit filter applied before sampling. `0` disables it (reference default).
    pub top_k: usize,
    /// Top-p (nucleus) logit filter applied before sampling. `1.0` disables it
    /// (reference default).
    pub top_p: f32,
    /// Seed for the multinomial sampler, so a given config decodes
    /// reproducibly even though the reference (torch) RNG stream differs.
    pub seed: u64,
}

impl Default for DiffusionGenerationConfig {
    fn default() -> Self {
        // Matches the reference README inference call.
        Self {
            gen_length: 1024,
            block_length: 32,
            denoising_steps: 32,
            dynamic_threshold: 0.95,
            temperature: 1.0,
            top_k: 0,
            top_p: 1.0,
            seed: 0,
        }
    }
}

fn proc_err(msg: &'static str, e: candle_core::Error) -> Error {
    candle_to_ocr_processing(crate::error::ProcessingStage::TensorOperation, msg, e)
}

pub struct MinerUDiffusion {
    device: Device,
    dtype: DType,
    cfg: MinerUDiffusionConfig,
    image_cfg: MinerUImageProcessorConfig,
    tokenizer: Tokenizer,
    vision: MinerUVisionModel,
    abstractor: VisionAbstractor,
    text: SdarModel,
    image_token_id: u32,
    mask_token_id: u32,
    merge_size: usize,
    eos_token_ids: Vec<u32>,
}

impl MinerUDiffusion {
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = MinerUDiffusionConfig::from_path(model_dir.join("config.json"))?;
        let mut image_cfg =
            MinerUImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;
        // MinerU-Diffusion ships both an explicit `min/max_pixels` pair and a
        // `size` block whose `longest_edge` (12845056) is the absolute ceiling,
        // not the operating cap. When both are present, prefer the explicit
        // pixel bounds (the Qwen2-VL `max_pixels`, 1605632) by dropping `size`
        // so oversized pages downscale to the intended budget.
        if image_cfg.min_pixels.is_some() && image_cfg.max_pixels.is_some() {
            image_cfg.size = None;
        }
        image_cfg.validate()?;

        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("failed to load MinerU-Diffusion tokenizer.json: {e}"),
            })?;

        if let Some(tok_image_id) = tokenizer.token_to_id("<|image_pad|>")
            && tok_image_id != cfg.image_token_id
        {
            return Err(Error::Config {
                message: format!(
                    "MinerU-Diffusion image_token_id mismatch: tokenizer {tok_image_id} != config {}",
                    cfg.image_token_id
                ),
            });
        }

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, "MinerU-Diffusion")?;
        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load safetensors", e))?
        };

        let merge_size = cfg.projector_merge_size()?;
        let vision = MinerUVisionModel::load_backbone(&cfg.vision_config, vb.pp("vision_model"))?;
        let abstractor = VisionAbstractor::load(
            vb.pp("vision_abstractor").pp("projection"),
            cfg.vision_config.embed_dim,
            cfg.text_config.hidden_size,
            merge_size,
        )?;
        let text = SdarModel::load(&cfg.text_config, vb.pp("language_model"), dtype)?;

        let mut eos_token_ids = Vec::new();
        if let Some(eos) = cfg.text_config.eos_token_id {
            eos_token_ids.push(eos);
        }
        // The model is trained to stop on <|im_end|>; include it explicitly.
        if let Some(im_end) = tokenizer.token_to_id("<|im_end|>") {
            eos_token_ids.push(im_end);
        }
        if let Some(eot) = tokenizer.token_to_id("<|endoftext|>") {
            eos_token_ids.push(eot);
        }
        eos_token_ids.sort_unstable();
        eos_token_ids.dedup();

        let image_token_id = cfg.image_token_id;
        let mask_token_id = cfg.mask_token_id;
        Ok(Self {
            device,
            dtype,
            cfg,
            image_cfg,
            tokenizer,
            vision,
            abstractor,
            text,
            image_token_id,
            mask_token_id,
            merge_size,
            eos_token_ids,
        })
    }

    /// Recognize a single image with the given instruction (e.g.
    /// [`DEFAULT_PROMPT`]). Returns the decoded text with special tokens
    /// stripped — suitable for element-level recognition (`Text`/`Table`/
    /// `Formula Recognition:`) where OTSL markers (`<fcel>`, `<nl>`) survive
    /// because they are ordinary vocab tokens, not special tokens.
    pub fn generate_one(
        &self,
        image: &RgbImage,
        instruction: &str,
        gen_cfg: &DiffusionGenerationConfig,
    ) -> Result<String, Error> {
        let tokens = self.generate_token_ids(image, instruction, gen_cfg)?;
        self.decode_tokens(&tokens, true)
    }

    /// Generate decoded text for a batch of image/instruction pairs.
    pub fn generate(
        &self,
        images: &[RgbImage],
        instructions: &[impl AsRef<str>],
        gen_cfg: &DiffusionGenerationConfig,
    ) -> crate::error::BatchResult<String> {
        if images.len() != instructions.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU-Diffusion: images count ({}) != instructions count ({})",
                    images.len(),
                    instructions.len()
                ),
            });
        }
        Ok(images
            .iter()
            .zip(instructions)
            .map(|(image, instruction)| self.generate_one(image, instruction.as_ref(), gen_cfg))
            .collect())
    }

    /// Same as [`Self::generate_one`] but preserves special tokens in the decoded
    /// string. Required for the `\nLayout Detection:` pass, whose output frames
    /// each region with `<|box_start|>`/`<|box_end|>`/`<|ref_start|>`/
    /// `<|ref_end|>` special tokens that the layout parser keys on.
    pub fn generate_raw(
        &self,
        image: &RgbImage,
        instruction: &str,
        gen_cfg: &DiffusionGenerationConfig,
    ) -> Result<String, Error> {
        let tokens = self.generate_token_ids(image, instruction, gen_cfg)?;
        self.decode_tokens(&tokens, false)
    }

    /// Run vision + block-diffusion decode and return the generated token ids,
    /// truncated at the first stop token (stop token excluded).
    fn generate_token_ids(
        &self,
        image: &RgbImage,
        instruction: &str,
        gen_cfg: &DiffusionGenerationConfig,
    ) -> Result<Vec<u32>, Error> {
        if gen_cfg.block_length == 0 || !gen_cfg.gen_length.is_multiple_of(gen_cfg.block_length) {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU-Diffusion: gen_length ({}) must be a positive multiple of block_length ({})",
                    gen_cfg.gen_length, gen_cfg.block_length
                ),
            });
        }

        // 1) Vision features.
        let image_inputs = preprocess_images(
            std::slice::from_ref(image),
            &self.image_cfg,
            &self.device,
            self.dtype,
        )?;
        let patches = self
            .vision
            .forward_tokens(&image_inputs.pixel_values, &image_inputs.image_grid_thw)?;
        let image_features = self.abstractor.forward(&patches)?;
        let num_image_tokens = image_features
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "image_features dim", e))?;

        // 2) Prompt -> token ids with the single image placeholder expanded.
        let prompt = build_prompt(instruction);
        let enc = self
            .tokenizer
            .encode(prompt, false)
            .map_err(|e| Error::InvalidInput {
                message: format!("MinerU-Diffusion: tokenizer encode failed: {e}"),
            })?;
        let input_ids = expand_image_tokens(enc.get_ids(), self.image_token_id, num_image_tokens)?;
        let prompt_len = input_ids.len();

        // 3) Embed prompt and splice image features into the placeholder span.
        let inputs_embeds = self.embed_with_image(&input_ids, &image_features, num_image_tokens)?;

        // 4) Block-diffusion decode.
        let mut rng = StdRng::seed_from_u64(gen_cfg.seed);
        let generated = self.run_diffusion(&inputs_embeds, prompt_len, gen_cfg, &mut rng)?;

        // 5) Truncate at the first stop token.
        let cut = generated
            .iter()
            .position(|id| self.eos_token_ids.contains(id))
            .unwrap_or(generated.len());
        Ok(generated[..cut].to_vec())
    }

    fn decode_tokens(&self, tokens: &[u32], skip_special: bool) -> Result<String, Error> {
        self.tokenizer
            .decode(tokens, skip_special)
            .map_err(|e| Error::InvalidInput {
                message: format!("MinerU-Diffusion: tokenizer decode failed: {e}"),
            })
    }

    fn embed_with_image(
        &self,
        input_ids: &[u32],
        image_features: &Tensor,
        num_image_tokens: usize,
    ) -> Result<Tensor, Error> {
        let seq_len = input_ids.len();
        let input_ids_t = Tensor::new(input_ids.to_vec(), &self.device)
            .and_then(|t| t.reshape((1, seq_len)))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "create input_ids", e))?;
        let mut embeds = self.text.embed(&input_ids_t)?;

        if let Some(first_pos) = input_ids.iter().position(|&id| id == self.image_token_id) {
            let image_end = first_pos + num_image_tokens;
            if image_end > seq_len {
                return Err(Error::InvalidInput {
                    message: format!(
                        "MinerU-Diffusion: image token span out of range: {image_end} > {seq_len}"
                    ),
                });
            }
            let image_features = image_features
                .to_dtype(self.dtype)
                .map_err(|e| proc_err("MinerU-Diffusion image_features dtype", e))?;
            let mut parts: Vec<Tensor> = Vec::with_capacity(3);
            if first_pos > 0 {
                parts.push(
                    embeds
                        .narrow(1, 0, first_pos)
                        .map_err(|e| proc_err("narrow prefix", e))?,
                );
            }
            parts.push(
                image_features
                    .unsqueeze(0)
                    .map_err(|e| proc_err("unsqueeze image", e))?,
            );
            if image_end < seq_len {
                parts.push(
                    embeds
                        .narrow(1, image_end, seq_len - image_end)
                        .map_err(|e| proc_err("narrow suffix", e))?,
                );
            }
            let refs: Vec<&Tensor> = parts.iter().collect();
            embeds = Tensor::cat(&refs, 1).map_err(|e| proc_err("cat embeds", e))?;
        }
        Ok(embeds)
    }

    fn run_diffusion(
        &self,
        inputs_embeds: &Tensor,
        prompt_len: usize,
        gen_cfg: &DiffusionGenerationConfig,
        rng: &mut StdRng,
    ) -> Result<Vec<u32>, Error> {
        let block = gen_cfg.block_length;
        let gen_blocks = gen_cfg.gen_length / block;
        let mut cache = SdarKvCache::new(self.text.num_layers());

        // Prefill the prompt with a block-causal mask.
        let prompt_mask = self.block_causal_mask(prompt_len, block)?;
        let prompt_positions: Vec<i64> = (0..prompt_len as i64).collect();
        self.text.forward(
            inputs_embeds,
            &prompt_positions,
            Some(&prompt_mask),
            &mut cache,
            true,
        )?;

        let num_transfer = num_transfer_tokens(block, gen_cfg.denoising_steps);
        let mut generated: Vec<u32> = Vec::with_capacity(gen_cfg.gen_length);

        for b in 0..gen_blocks {
            let block_start = prompt_len + b * block;
            let positions: Vec<i64> = (block_start as i64..(block_start + block) as i64).collect();
            // Every block position starts masked; decoded tokens overwrite it.
            let mut cur_tokens = vec![self.mask_token_id; block];

            let mut committed = false;
            for step in 0..=gen_cfg.denoising_steps {
                let masked: Vec<usize> = cur_tokens
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &t)| (t == self.mask_token_id).then_some(i))
                    .collect();

                let cur_embeds = self.embed_tokens_block(&cur_tokens)?;
                if masked.is_empty() {
                    // Commit the fully-decoded block's KV and move on.
                    self.text
                        .forward(&cur_embeds, &positions, None, &mut cache, true)?;
                    committed = true;
                    break;
                }

                let hidden = self
                    .text
                    .forward(&cur_embeds, &positions, None, &mut cache, false)?;
                let logits = self.text.lm_logits(&hidden)?;
                let (tokens, confs) = sample_tokens_and_conf(
                    &logits,
                    gen_cfg.temperature,
                    gen_cfg.top_k,
                    gen_cfg.top_p,
                    rng,
                )?;

                // `num_transfer` always has at least one entry (steps is clamped
                // to >= 1 in `num_transfer_tokens`); `saturating_sub` keeps the
                // index in range when `denoising_steps == 0`.
                let want = num_transfer[step.min(gen_cfg.denoising_steps.saturating_sub(1))];
                let transfer = select_transfer(&masked, &confs, gen_cfg.dynamic_threshold, want);
                for pos in transfer {
                    cur_tokens[pos] = tokens[pos];
                }
            }

            // Fallback commit: the in-loop commit only fires when a later
            // iteration observes a fully-unmasked block, which never happens
            // when the schedule unmasks everything on the final step (e.g.
            // `denoising_steps == 0`, or an early exit). Without this the block's
            // KV is missing from the cache and the next block prefills on stale
            // state.
            if !committed {
                let cur_embeds = self.embed_tokens_block(&cur_tokens)?;
                self.text
                    .forward(&cur_embeds, &positions, None, &mut cache, true)?;
            }

            generated.extend_from_slice(&cur_tokens);
            if cur_tokens.iter().any(|id| self.eos_token_ids.contains(id)) {
                break;
            }
        }
        Ok(generated)
    }

    fn embed_tokens_block(&self, tokens: &[u32]) -> Result<Tensor, Error> {
        let t = Tensor::new(tokens.to_vec(), &self.device)
            .and_then(|t| t.reshape((1, tokens.len())))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "block tokens tensor", e))?;
        self.text.embed(&t)
    }

    /// Additive block-causal mask `(1, 1, P, P)`: blocks of `block` tokens are
    /// causal at block granularity (full attention within/earlier blocks),
    /// aligned to the END of the prompt — matching the reference construction.
    fn block_causal_mask(&self, prompt_len: usize, block: usize) -> Result<Tensor, Error> {
        let data = block_causal_mask_data(prompt_len, block);
        Tensor::from_vec(data, (1, 1, prompt_len, prompt_len), &self.device)
            .and_then(|t| t.to_dtype(self.dtype))
            .map_err(|e| proc_err("block-causal mask", e))
    }

    pub fn merge_size(&self) -> usize {
        self.merge_size
    }

    pub fn config(&self) -> &MinerUDiffusionConfig {
        &self.cfg
    }
}

impl RecognitionBackend for MinerUDiffusion {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        if max_tokens == 0 {
            return Ok(String::new());
        }
        let prompt = match task {
            RecognitionTask::Ocr => "\nText Recognition:",
            RecognitionTask::Table => "\nTable Recognition:",
            RecognitionTask::Formula => "\nFormula Recognition:",
            RecognitionTask::Chart => "\nDocument Parsing:",
        };
        let mut generation = DiffusionGenerationConfig::default();
        generation.gen_length = max_tokens
            .checked_next_multiple_of(generation.block_length)
            .ok_or_else(|| Error::InvalidInput {
                message: format!("MinerU-Diffusion max_tokens {max_tokens} is too large"),
            })?;
        let mut tokens = self.generate_token_ids(&image, prompt, &generation)?;
        // Diffusion generates whole blocks, but the backend contract exposes a
        // hard token limit to DocParser callers.
        tokens.truncate(max_tokens);
        self.decode_tokens(&tokens, true)
    }

    fn needs_table_postprocess(&self) -> bool {
        true
    }

    fn needs_repetition_truncation(&self) -> bool {
        true
    }
}

fn build_prompt(instruction: &str) -> String {
    // The MinerU-Diffusion chat template emits no inter-segment newlines.
    format!(
        "<|im_start|>systemYou are a helpful assistant.<|im_end|><|im_start|>user<|vision_start|><|image_pad|><|vision_end|>{instruction}<|im_end|><|im_start|>assistant"
    )
}

fn expand_image_tokens(ids: &[u32], image_token: u32, n: usize) -> Result<Vec<u32>, Error> {
    let count = ids.iter().filter(|&&id| id == image_token).count();
    if count != 1 {
        return Err(Error::InvalidInput {
            message: format!(
                "MinerU-Diffusion: expected exactly one image placeholder token, found {count}"
            ),
        });
    }
    let mut out = Vec::with_capacity(ids.len() + n.saturating_sub(1));
    for &id in ids {
        if id == image_token {
            out.extend(std::iter::repeat_n(image_token, n));
        } else {
            out.push(id);
        }
    }
    Ok(out)
}

/// Reference `get_num_transfer_tokens`: spread `block` unmaskings across
/// `steps`, distributing the remainder to the earliest steps.
fn num_transfer_tokens(block: usize, steps: usize) -> Vec<usize> {
    let steps = steps.max(1);
    let base = block / steps;
    let remainder = block % steps;
    (0..steps)
        .map(|i| base + usize::from(i < remainder))
        .collect()
}

/// Per block position, sample a token and return its probability — the
/// reference `sample_with_temperature_topk_topp` (temperature scaling, optional
/// top-k / top-p filtering, then `torch.multinomial`). The returned confidence
/// is the probability of the *sampled* token (reference `x0_p`), not the argmax
/// probability. `temperature <= 0.0` is treated as deterministic greedy
/// (argmax) decoding, which ignores `rng`/`top_k`/`top_p`.
fn sample_tokens_and_conf(
    logits: &Tensor,
    temperature: f32,
    top_k: usize,
    top_p: f32,
    rng: &mut StdRng,
) -> Result<(Vec<u32>, Vec<f32>), Error> {
    #[cfg(feature = "cuda")]
    if matches!(logits.device(), Device::Cuda(_))
        && matches!(logits.dtype(), DType::BF16 | DType::F32)
        && top_k == 0
        && top_p >= 1.0
        && std::env::var_os("OAR_MINERU_DIFFUSION_DISABLE_GPU_SAMPLING").is_none()
    {
        return sample_tokens_and_conf_cuda(logits, temperature, rng);
    }

    // logits: (1, block, vocab) -> rows of (vocab,)
    let logits = logits
        .squeeze(0)
        .map_err(|e| proc_err("logits squeeze", e))?
        .to_dtype(DType::F32)
        .map_err(|e| proc_err("logits f32", e))?;
    let rows = logits
        .to_vec2::<f32>()
        .map_err(|e| proc_err("logits to_vec2", e))?;

    let greedy = temperature <= 0.0;
    let mut tokens = Vec::with_capacity(rows.len());
    let mut confs = Vec::with_capacity(rows.len());

    for mut row in rows {
        if greedy {
            let idx = argmax_index(&row);
            let probs = softmax(&row);
            tokens.push(idx as u32);
            confs.push(probs[idx]);
            continue;
        }

        // Reference order: temperature, then top_k, then top_p, then softmax.
        if (temperature - 1.0).abs() > f32::EPSILON {
            for v in row.iter_mut() {
                *v /= temperature;
            }
        }
        if top_k > 0 {
            apply_top_k(&mut row, top_k);
        }
        if top_p < 1.0 {
            apply_top_p(&mut row, top_p);
        }
        let probs = softmax(&row);
        let idx = match WeightedIndex::new(&probs) {
            Ok(dist) => dist.sample(rng),
            // Degenerate distribution (all zero / NaN) -> fall back to argmax.
            Err(_) => argmax_index(&probs),
        };
        tokens.push(idx as u32);
        confs.push(probs[idx]);
    }
    Ok((tokens, confs))
}

/// Fast path for the reference/default sampling configuration. The model's
/// logits remain on the GPU; only two F32 values per block position (token id
/// and selected probability) cross to the host. The CPU fallback above stays
/// available for top-k/top-p filters and as a numerical/debugging oracle via
/// `OAR_MINERU_DIFFUSION_DISABLE_GPU_SAMPLING=1`.
#[cfg(feature = "cuda")]
fn sample_tokens_and_conf_cuda(
    logits: &Tensor,
    temperature: f32,
    rng: &mut StdRng,
) -> Result<(Vec<u32>, Vec<f32>), Error> {
    let logits = logits
        .squeeze(0)
        .and_then(|t| t.contiguous())
        .map_err(|e| proc_err("CUDA sampling logits", e))?;
    let rows = logits
        .dim(0)
        .map_err(|e| proc_err("CUDA sampling rows", e))?;
    let greedy = temperature <= 0.0;
    let uniforms = if greedy {
        vec![0.0f32; rows]
    } else {
        (0..rows).map(|_| rng.random::<f32>()).collect()
    };
    let uniforms = Tensor::from_vec(uniforms, rows, logits.device())
        .map_err(|e| proc_err("CUDA sampling uniforms", e))?;
    let sampled = logits
        .apply_op2_no_bwd(
            &uniforms,
            &SampleWithConfidence {
                temperature,
                greedy,
            },
        )
        .and_then(|t| t.to_vec2::<f32>())
        .map_err(|e| proc_err("CUDA sample-with-confidence", e))?;

    let mut tokens = Vec::with_capacity(rows);
    let mut confs = Vec::with_capacity(rows);
    for row in sampled {
        tokens.push(row[0] as u32);
        confs.push(row[1]);
    }
    Ok((tokens, confs))
}

/// Numerically stable softmax over a logits row.
fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        // All -inf (shouldn't happen post-filter) -> uniform to stay well-defined.
        let n = logits.len().max(1) as f32;
        return vec![1.0 / n; logits.len()];
    }
    let mut exps: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    if sum > 0.0 {
        for e in exps.iter_mut() {
            *e /= sum;
        }
    }
    exps
}

fn argmax_index(values: &[f32]) -> usize {
    let mut best = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (i, &v) in values.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best = i;
        }
    }
    best
}

/// Reference `top_k_logits`: keep the `top_k` largest logits, set the rest to
/// `-inf`.
fn apply_top_k(logits: &mut [f32], top_k: usize) {
    if top_k == 0 || top_k >= logits.len() {
        return;
    }
    // Only the top-`top_k`/rest partition matters, not the full order, so a
    // linear-time partial selection beats an O(N log N) sort over the whole
    // vocabulary (~152k entries) on every denoising step.
    let mut order: Vec<usize> = (0..logits.len()).collect();
    order.select_nth_unstable_by(top_k, |&a, &b| {
        logits[b]
            .partial_cmp(&logits[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for &i in order.iter().skip(top_k) {
        logits[i] = f32::NEG_INFINITY;
    }
}

/// Reference `top_p_logits`: keep the smallest set of highest-probability tokens
/// whose cumulative probability first reaches `top_p` (the token that crosses
/// the threshold is kept; the top token is always kept), masking the rest to
/// `-inf`. The cumulative probabilities use the softmax of the *pre-mask*
/// logits, matching the reference.
fn apply_top_p(logits: &mut [f32], top_p: f32) {
    let probs = softmax(logits);
    let mut order: Vec<usize> = (0..logits.len()).collect();
    order.sort_by(|&a, &b| {
        logits[b]
            .partial_cmp(&logits[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut cum_before = 0.0f32;
    for (rank, &i) in order.iter().enumerate() {
        let keep = rank == 0 || cum_before <= top_p;
        if !keep {
            logits[i] = f32::NEG_INFINITY;
        }
        cum_before += probs[i];
    }
}

/// Select which masked positions to unmask this step: all above `threshold`,
/// but at least `want` (the highest-confidence masked positions).
fn select_transfer(masked: &[usize], confs: &[f32], threshold: f32, want: usize) -> Vec<usize> {
    let above: Vec<usize> = masked
        .iter()
        .copied()
        .filter(|&p| confs[p] > threshold)
        .collect();
    if above.len() >= want.max(1) {
        return above;
    }
    let mut ranked = masked.to_vec();
    ranked.sort_by(|&a, &b| {
        confs[b]
            .partial_cmp(&confs[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let take = want.max(1).min(ranked.len());
    ranked.truncate(take);
    ranked
}

/// Additive block-causal mask data `(prompt_len * prompt_len)`, row-major.
/// Blocks of `block` tokens are causal at block granularity (full attention
/// within and to earlier blocks), aligned to the END of the prompt so the last
/// block boundary lands on `prompt_len` — matching the reference construction.
/// A `NEG_INFINITY` entry at `[i, j]` masks key `j` from query `i`.
fn block_causal_mask_data(prompt_len: usize, block: usize) -> Vec<f32> {
    let prompt_blocks = prompt_len.div_ceil(block);
    let offset = prompt_blocks * block - prompt_len;
    let block_of = |x: usize| (offset + x) / block;
    let mut data = vec![0f32; prompt_len * prompt_len];
    for i in 0..prompt_len {
        let bi = block_of(i);
        for j in 0..prompt_len {
            if block_of(j) > bi {
                data[i * prompt_len + j] = f32::NEG_INFINITY;
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_matches_reference_chat_template() {
        // Locked against `chat_template.jinja` from
        // `MinerU-Diffusion-V1-0320-2.5B`: system -> user(image,text) ->
        // assistant, with NO inter-segment newlines (the template emits none).
        // Rendering that template with the README's messages yields exactly:
        let expected = "<|im_start|>systemYou are a helpful assistant.<|im_end|>\
             <|im_start|>user<|vision_start|><|image_pad|><|vision_end|>\
             \nText Recognition:<|im_end|><|im_start|>assistant";
        assert_eq!(build_prompt(DEFAULT_PROMPT), expected);
    }

    #[test]
    fn num_transfer_tokens_sums_to_block() {
        // Reference get_num_transfer_tokens: base everywhere, +1 on the first
        // `remainder` steps. The schedule must unmask exactly `block` positions.
        let sched = num_transfer_tokens(32, 32);
        assert_eq!(sched, vec![1; 32]);
        assert_eq!(sched.iter().sum::<usize>(), 32);

        let sched = num_transfer_tokens(32, 10);
        // 32 = 3*10 + 2 -> first two steps get 4, the rest 3.
        assert_eq!(sched, vec![4, 4, 3, 3, 3, 3, 3, 3, 3, 3]);
        assert_eq!(sched.iter().sum::<usize>(), 32);
    }

    #[test]
    fn num_transfer_tokens_handles_zero_steps() {
        // steps is clamped to >= 1 to avoid a divide-by-zero.
        assert_eq!(num_transfer_tokens(8, 0), vec![8]);
    }

    #[test]
    fn select_transfer_takes_all_above_threshold() {
        // When at least `want` positions clear the threshold, take all of them.
        let masked = vec![0, 1, 2, 3];
        let confs = vec![0.99, 0.5, 0.97, 0.96];
        let mut got = select_transfer(&masked, &confs, 0.95, 1);
        got.sort_unstable();
        assert_eq!(got, vec![0, 2, 3]);
    }

    #[test]
    fn select_transfer_falls_back_to_top_want() {
        // Nothing clears the threshold -> take the `want` highest-confidence
        // masked positions (here the single best).
        let masked = vec![0, 1, 2, 3];
        let confs = vec![0.1, 0.9, 0.3, 0.2];
        assert_eq!(select_transfer(&masked, &confs, 0.95, 1), vec![1]);
    }

    #[test]
    fn select_transfer_only_considers_masked_positions() {
        // Position 1 has the highest confidence but is not masked, so it must
        // never be selected even though confs[1] > threshold.
        let masked = vec![0, 2];
        let confs = vec![0.5, 0.99, 0.97, 0.5];
        assert_eq!(select_transfer(&masked, &confs, 0.95, 1), vec![2]);
    }

    #[test]
    fn select_transfer_forces_at_least_one() {
        // want == 0 still commits the single best masked position, guaranteeing
        // forward progress (this is the deliberate divergence from the
        // reference, which permits zero transfers).
        let masked = vec![0, 1];
        let confs = vec![0.1, 0.2];
        assert_eq!(select_transfer(&masked, &confs, 0.95, 0), vec![1]);
    }

    #[test]
    fn expand_image_tokens_expands_single_placeholder() {
        // The lone image token is replaced by `n` copies; surrounding ids stay.
        let ids = vec![10, 11, 99, 12];
        let out = expand_image_tokens(&ids, 99, 3).unwrap();
        assert_eq!(out, vec![10, 11, 99, 99, 99, 12]);
    }

    #[test]
    fn expand_image_tokens_rejects_wrong_count() {
        assert!(expand_image_tokens(&[10, 11, 12], 99, 3).is_err());
        assert!(expand_image_tokens(&[99, 11, 99], 99, 3).is_err());
    }

    #[test]
    fn block_causal_mask_is_block_granular_and_end_aligned() {
        // prompt_len=3, block=2 -> 2 prompt blocks, offset = 4 - 3 = 1, so the
        // block boundary lands so the LAST block ends at prompt_len. Token 0 is
        // alone in block 0; tokens 1,2 share block 1.
        let m = block_causal_mask_data(3, 2);
        let at = |i: usize, j: usize| m[i * 3 + j];
        let neg = f32::NEG_INFINITY;
        // Query 0 (block 0) cannot see tokens 1,2 (block 1).
        assert_eq!(at(0, 0), 0.0);
        assert_eq!(at(0, 1), neg);
        assert_eq!(at(0, 2), neg);
        // Queries 1 and 2 (block 1) see everything (block 0 and their own block).
        for j in 0..3 {
            assert_eq!(at(1, j), 0.0);
            assert_eq!(at(2, j), 0.0);
        }
    }

    #[test]
    fn softmax_is_stable_and_normalized() {
        let p = softmax(&[2.0, 1.0, 0.0]);
        let sum: f32 = p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        // Monotonic with logits and matches the closed form.
        assert!((p[0] - 0.665240).abs() < 1e-4);
        assert!(p[0] > p[1] && p[1] > p[2]);
        // Large logits must not overflow (stability subtracts the max).
        let big = softmax(&[1000.0, 999.0]);
        assert!(big.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn apply_top_k_keeps_only_the_k_largest() {
        // Reference top_k_logits: everything outside the top-k becomes -inf.
        let mut l = vec![0.0, 3.0, 1.0, 2.0];
        apply_top_k(&mut l, 2);
        assert_eq!(l[1], 3.0);
        assert_eq!(l[3], 2.0);
        assert_eq!(l[0], f32::NEG_INFINITY);
        assert_eq!(l[2], f32::NEG_INFINITY);
        // k >= len is a no-op.
        let mut l2 = vec![0.0, 1.0];
        apply_top_k(&mut l2, 5);
        assert_eq!(l2, vec![0.0, 1.0]);
    }

    #[test]
    fn apply_top_p_keeps_threshold_crossing_token() {
        // probs([2,1,0]) ~= [0.665, 0.245, 0.090].
        // p=0.8: keep ranks until cumulative-before exceeds 0.8 -> keep 0 and 1,
        // drop 2 (cum-before at rank 2 = 0.910 > 0.8).
        let mut l = vec![2.0, 1.0, 0.0];
        apply_top_p(&mut l, 0.8);
        assert_eq!(l[0], 2.0);
        assert_eq!(l[1], 1.0);
        assert_eq!(l[2], f32::NEG_INFINITY);
        // p=0.6: the top token alone already exceeds 0.6, but rank 0 is always
        // kept; ranks 1 and 2 are dropped.
        let mut l2 = vec![2.0, 1.0, 0.0];
        apply_top_p(&mut l2, 0.6);
        assert_eq!(l2[0], 2.0);
        assert_eq!(l2[1], f32::NEG_INFINITY);
        assert_eq!(l2[2], f32::NEG_INFINITY);
    }

    #[test]
    fn sample_greedy_returns_argmax_and_its_prob() {
        // temperature <= 0 -> deterministic argmax with the softmax prob as
        // confidence (matches the old greedy decode path).
        let logits = Tensor::from_vec(vec![2.0f32, 1.0, 0.0], (1, 1, 3), &Device::Cpu).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        let (toks, confs) = sample_tokens_and_conf(&logits, 0.0, 0, 1.0, &mut rng).unwrap();
        assert_eq!(toks, vec![0]);
        assert!((confs[0] - 0.665240).abs() < 1e-4);
    }

    #[test]
    fn sample_is_reproducible_for_a_fixed_seed() {
        // Same seed + same logits -> identical samples (so a decode is
        // reproducible even though the reference torch RNG stream differs).
        let logits =
            Tensor::from_vec(vec![1.0f32, 1.0, 1.0, 1.0], (1, 2, 2), &Device::Cpu).unwrap();
        let run = || {
            let mut rng = StdRng::seed_from_u64(42);
            sample_tokens_and_conf(&logits, 1.0, 0, 1.0, &mut rng)
                .unwrap()
                .0
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn block_causal_mask_exact_multiple_no_offset() {
        // prompt_len=4, block=2 -> offset 0, two clean blocks {0,1} and {2,3}.
        let m = block_causal_mask_data(4, 2);
        let at = |i: usize, j: usize| m[i * 4 + j];
        let neg = f32::NEG_INFINITY;
        // Block 0 queries (0,1) masked from block 1 keys (2,3).
        assert_eq!(at(0, 2), neg);
        assert_eq!(at(1, 3), neg);
        // Within-block and earlier-block attention is allowed.
        assert_eq!(at(1, 0), 0.0);
        assert_eq!(at(3, 1), 0.0);
        assert_eq!(at(3, 3), 0.0);
    }
}
