use super::config::{EosTokenId, GlmOcrConfig, GlmOcrImageProcessorConfig};
#[cfg(feature = "cuda")]
use super::mtp::GlmOcrMtpModel;
use super::processing::{GlmOcrImageInputs, preprocess_image};
use super::text::GlmOcrTextModel;
use super::vision::GlmOcrVisionModel;
#[cfg(feature = "cuda")]
use crate::decoder_graph::CudaGraphDrainGuard;
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{Linear, Module, VarBuilder, linear_no_bias};
use image::RgbImage;
use std::path::Path;
use tokenizers::Tokenizer;

#[cfg(feature = "cuda")]
// Four recurrent proposals are fastest on the reference 4090 across text,
// formula, table, and long document outputs. The upstream-recommended three
// leaves useful acceptance on the table for Candle's batch-1 path.
const GLM_MTP_DRAFT_TOKENS: usize = 4;
#[cfg(feature = "cuda")]
const GLM_MTP_QUERY_LEN: usize = GLM_MTP_DRAFT_TOKENS + 1;

pub struct GlmOcr {
    device: Device,
    dtype: DType,
    cfg: GlmOcrConfig,
    image_cfg: GlmOcrImageProcessorConfig,
    tokenizer: Tokenizer,
    text: GlmOcrTextModel,
    #[cfg(feature = "cuda")]
    mtp: Option<GlmOcrMtpModel>,
    vision: GlmOcrVisionModel,
    lm_head: Linear,
    eos_token_ids: Vec<u32>,
    image_token_id: u32,
    // Must stay the last field: the captured decoder graphs read the LM head
    // owned here, so this guard drops last and drains CUDA errors the head's
    // free may stash (see CudaGraphDrainGuard).
    #[cfg(feature = "cuda")]
    _drain_guard: CudaGraphDrainGuard,
}

impl GlmOcr {
    pub fn from_dir(model_dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let model_dir = model_dir.as_ref();
        let cfg = GlmOcrConfig::from_path(model_dir.join("config.json"))?;
        let image_cfg =
            GlmOcrImageProcessorConfig::from_path(model_dir.join("preprocessor_config.json"))?;

        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(|e| Error::Config {
                message: format!("failed to load GLM-OCR tokenizer.json: {e}"),
            })?;

        if let Some(tok_image_id) = tokenizer.token_to_id("<|image|>")
            && tok_image_id != cfg.image_token_id
        {
            return Err(Error::Config {
                message: format!(
                    "GLM-OCR image_token_id mismatch: tokenizer {tok_image_id} != config {}",
                    cfg.image_token_id
                ),
            });
        }

        let dtype = crate::utils::select_dtype(&device);
        let weight_files = crate::utils::collect_safetensors(model_dir, "GLM-OCR")?;
        // SAFETY: The mmap'd files must not be modified or deleted while in use.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load safetensors", e))?
        };

        let image_token_id = cfg.image_token_id;
        let vb_language_model = vb.pp("model").pp("language_model");
        let text = GlmOcrTextModel::load(&cfg.text_config, vb_language_model.clone())?;
        #[cfg(feature = "cuda")]
        let mtp = if device.is_cuda()
            && cfg.text_config.num_nextn_predict_layers == 1
            && std::env::var_os("OAR_GLMOCR_DISABLE_MTP").is_none()
            && std::env::var_os("OAR_VL_DISABLE_SPECULATIVE").is_none()
        {
            Some(GlmOcrMtpModel::load(
                &cfg.text_config,
                cfg.text_config.num_hidden_layers,
                vb_language_model,
            )?)
        } else {
            None
        };
        let vision = GlmOcrVisionModel::load(&cfg.vision_config, vb.pp("model").pp("visual"))?;

        let lm_head = if cfg.text_config.tie_word_embeddings || cfg.tie_word_embeddings {
            Linear::new(text.token_embedding_weight(), None)
        } else {
            linear_no_bias(
                cfg.text_config.hidden_size,
                cfg.text_config.vocab_size,
                vb.pp("lm_head"),
            )
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load lm_head", e))?
        };

        let eos_token_ids = match &cfg.text_config.eos_token_id {
            EosTokenId::Single(v) => vec![*v],
            EosTokenId::Multiple(v) => v.clone(),
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
            #[cfg(feature = "cuda")]
            mtp,
            vision,
            lm_head,
            eos_token_ids,
            image_token_id,
            #[cfg(feature = "cuda")]
            _drain_guard,
        })
    }

    /// Generate OCR output for one or more images with optional instructions.
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
                    "GLM-OCR: images count ({}) != instructions count ({})",
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

    /// Generate raw token ids without post-processing. Tokens are exactly the
    /// ids emitted by the decode loop, excluding stop tokens, before tokenizer
    /// decoding or repetition truncation.
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
                    "GLM-OCR: images count ({}) != instructions count ({})",
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
        let mut results = Vec::with_capacity(images.len());

        for (image, instruction) in images.iter().zip(instructions.iter()) {
            let image_inputs = preprocess_image(
                image,
                &self.image_cfg,
                &self.cfg.vision_config,
                &self.device,
                self.dtype,
            )?;

            let prompt = build_prompt(instruction.as_ref());
            let prompt = expand_image_tokens(&prompt, image_inputs.num_image_tokens)?;

            let enc = self
                .tokenizer
                .encode(prompt, false)
                .map_err(|e| Error::InvalidInput {
                    message: format!("GLM-OCR: tokenizer encode failed: {e}"),
                })?;
            let input_ids = enc.get_ids().to_vec();
            if input_ids.is_empty() {
                return Err(Error::InvalidInput {
                    message: "GLM-OCR: empty prompt after tokenization".to_string(),
                });
            }

            let seq_len = input_ids.len();
            let inputs_embeds = self.prepare_inputs(&input_ids, &image_inputs)?;
            let (position_ids, max_pos) = build_position_ids(
                &input_ids,
                image_inputs.grid_thw,
                self.cfg.vision_config.spatial_merge_size,
                self.image_token_id,
                &self.device,
            )?;
            let rope_delta = max_pos + 1 - seq_len as i64;

            self.text.clear_kv_cache();
            #[cfg(feature = "cuda")]
            let use_mtp = self.mtp.is_some()
                && self.dtype == DType::BF16
                && max_new_tokens >= 8
                && std::env::var_os("OAR_VL_DISABLE_SPECULATIVE").is_none();
            #[cfg(not(feature = "cuda"))]
            let use_mtp = false;

            #[cfg(feature = "cuda")]
            if use_mtp {
                let mtp = self.mtp.as_ref().expect("MTP availability checked");
                mtp.clear_kv_cache();
                if let Some(cache_len) = self.text.prepare_verification_cuda_graph(
                    seq_len,
                    max_new_tokens,
                    GLM_MTP_QUERY_LEN,
                    &self.lm_head,
                )? {
                    mtp.prepare_cuda_graph(cache_len)?;
                } else {
                    // An eager prefill may grow/reallocate MTP storage. Any
                    // graph captured for an earlier page would then retain
                    // stale device pointers.
                    mtp.disable_cuda_graph();
                }
            }
            if !use_mtp {
                self.text
                    .prepare_ar_cuda_graph(seq_len, max_new_tokens, &self.lm_head)?;
            }

            let hidden = self.text.forward(&inputs_embeds, &position_ids, None)?;
            #[cfg(feature = "cuda")]
            if use_mtp {
                let generated = self.generate_mtp_tokens(
                    &input_ids,
                    &position_ids,
                    &hidden,
                    rope_delta,
                    max_new_tokens,
                    self.mtp.as_ref().expect("MTP availability checked"),
                )?;
                results.push(generated);
                continue;
            }

            let generated =
                self.generate_ar_tokens(&hidden, seq_len, rope_delta, max_new_tokens)?;

            results.push(generated);
        }

        Ok(results)
    }

    fn generate_ar_tokens(
        &self,
        prompt_hidden: &Tensor,
        seq_len: usize,
        rope_delta: i64,
        max_new_tokens: usize,
    ) -> Result<Vec<u32>, Error> {
        let last = prompt_hidden.i((0, seq_len - 1, ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: get last hidden",
                e,
            )
        })?;
        let mut logits = self.logits_from_hidden(&last)?;
        let mut generated = Vec::with_capacity(max_new_tokens);

        for (step, pos) in (seq_len as i64..).take(max_new_tokens).enumerate() {
            let tok = logits
                .argmax(D::Minus1)
                .and_then(|t| t.to_scalar::<u32>())
                .map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "GLM-OCR: argmax",
                        e,
                    )
                })?;
            if self.eos_token_ids.contains(&tok) {
                break;
            }
            generated.push(tok);
            if step + 1 == max_new_tokens {
                break;
            }

            let token = token_tensor(tok, &self.device)?;
            let embed = self.text.embed(&token)?;
            let pos_ids = text_position_ids(pos + rope_delta, 1, &self.device)?;
            logits = self
                .text
                .forward_decode_logits(&embed, &pos_ids, &self.lm_head)?;
        }
        Ok(generated)
    }

    #[cfg(feature = "cuda")]
    fn generate_mtp_tokens(
        &self,
        prompt_ids: &[u32],
        prompt_position_ids: &Tensor,
        prompt_hidden: &Tensor,
        rope_delta: i64,
        max_new_tokens: usize,
        mtp: &GlmOcrMtpModel,
    ) -> Result<Vec<u32>, Error> {
        let prompt_len = prompt_ids.len();
        let last = prompt_hidden.i((0, prompt_len - 1, ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: get MTP target hidden",
                e,
            )
        })?;
        let mut current_token = self
            .logits_from_hidden(&last)?
            .argmax(D::Minus1)
            .and_then(|token| token.to_scalar::<u32>())
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: initial MTP target argmax",
                    e,
                )
            })?;
        if max_new_tokens == 0 || self.eos_token_ids.contains(&current_token) {
            return Ok(Vec::new());
        }

        // Match vLLM's EAGLE/MTP alignment: shift prompt token ids left,
        // insert the target's certain next token at the tail, and pair that
        // sequence with the target hidden states at the original positions.
        let mut shifted_ids = Vec::with_capacity(prompt_len);
        shifted_ids.extend_from_slice(&prompt_ids[1..]);
        shifted_ids.push(current_token);
        let shifted_ids = Tensor::from_vec(shifted_ids, (1, prompt_len), &self.device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "initial MTP shifted ids", e))?;
        let (mtp_hidden, first_draft) =
            mtp.sync_target_span(&shifted_ids, prompt_hidden, prompt_position_ids, true)?;

        let mut target_position = prompt_len as i64 + rope_delta;
        let mut drafts =
            self.complete_mtp_drafts(mtp, &mtp_hidden, &first_draft, target_position)?;
        let mut target_cache_len = prompt_len;
        let mut generated = Vec::with_capacity(max_new_tokens);
        let mut rounds = 0usize;
        let mut accepted_drafts = 0usize;
        let mut accepted_by_position = [0usize; GLM_MTP_DRAFT_TOKENS];

        loop {
            if self.eos_token_ids.contains(&current_token) {
                break;
            }
            generated.push(current_token);
            if generated.len() == max_new_tokens {
                break;
            }

            let mut query_ids = Vec::with_capacity(GLM_MTP_QUERY_LEN);
            query_ids.push(current_token);
            query_ids.extend_from_slice(&drafts);
            let query =
                Tensor::from_vec(query_ids.clone(), (1, GLM_MTP_QUERY_LEN), &self.device)
                    .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP verification ids", e))?;
            let query_embeds = self.text.embed(&query)?;
            let query_positions =
                text_position_ids(target_position, GLM_MTP_QUERY_LEN, &self.device)?;
            let (target_hidden, target_tokens) = self.text.forward_verification_tokens(
                &query_embeds,
                &query_positions,
                &self.lm_head,
            )?;
            let target_tokens = target_tokens.to_vec1::<u32>().map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "copy MTP verification tokens", e)
            })?;
            if target_tokens.len() != GLM_MTP_QUERY_LEN {
                return Err(Error::InvalidInput {
                    message: format!(
                        "GLM-OCR: verification returned {} tokens, expected {}",
                        target_tokens.len(),
                        GLM_MTP_QUERY_LEN
                    ),
                });
            }

            rounds += 1;
            let mut accepted = 0usize;
            let mut stop = false;
            while accepted < GLM_MTP_DRAFT_TOKENS && drafts[accepted] == target_tokens[accepted] {
                let token = drafts[accepted];
                accepted += 1;
                accepted_drafts += 1;
                accepted_by_position[accepted - 1] += 1;
                if self.eos_token_ids.contains(&token) {
                    stop = true;
                    break;
                }
                generated.push(token);
                if generated.len() == max_new_tokens {
                    stop = true;
                    break;
                }
            }
            if stop {
                break;
            }

            let next_token = target_tokens[accepted];
            let keep = accepted + 1;

            // Target verification writes the whole block. Retain only the
            // certain current token and its accepted draft prefix. The MTP
            // recurrent tail is speculative too, so roll it back to the same
            // pre-query base before synchronizing the accepted target span.
            self.text.trim_kv_cache(target_cache_len + keep)?;
            mtp.trim_kv_cache(target_cache_len)?;
            target_cache_len += keep;
            target_position += keep as i64;
            current_token = next_token;

            if self.eos_token_ids.contains(&current_token) {
                break;
            }

            let mut shifted_sync_ids = query_ids[1..keep].to_vec();
            shifted_sync_ids.push(current_token);
            let shifted_sync_ids = Tensor::from_vec(shifted_sync_ids, (1, keep), &self.device)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP sync ids", e))?;
            let sync_hidden = target_hidden
                .narrow(1, 0, keep)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP sync target hidden", e))?;
            let sync_positions = query_positions
                .narrow(2, 0, keep)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP sync positions", e))?;
            let (mtp_hidden, first_draft) =
                mtp.sync_target_span(&shifted_sync_ids, &sync_hidden, &sync_positions, false)?;
            drafts = self.complete_mtp_drafts(mtp, &mtp_hidden, &first_draft, target_position)?;
        }

        if rounds > 0 {
            tracing::debug!(
                rounds,
                accepted_drafts,
                ?accepted_by_position,
                mean_acceptance_length = 1.0 + accepted_drafts as f64 / rounds as f64,
                "GLM-OCR MTP acceptance"
            );
        }
        Ok(generated)
    }

    #[cfg(feature = "cuda")]
    fn complete_mtp_drafts(
        &self,
        mtp: &GlmOcrMtpModel,
        first_hidden: &Tensor,
        first_tokens: &Tensor,
        target_position: i64,
    ) -> Result<Vec<u32>, Error> {
        let seq_len = first_hidden
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP result length", e))?;
        let mut hidden = first_hidden
            .narrow(1, seq_len - 1, 1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP final hidden state", e))?;
        let token_count = first_tokens
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP token count", e))?;
        let mut token = first_tokens
            .narrow(0, token_count - 1, 1)
            .and_then(|token| token.reshape((1, 1)))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP first proposal", e))?;
        let mut draft_tensors = Vec::with_capacity(GLM_MTP_DRAFT_TOKENS);

        while draft_tensors.len() < GLM_MTP_DRAFT_TOKENS {
            // CUDA-graph replay overwrites its captured output storage. Keep
            // each proposal in independent storage before launching the next
            // recurrent step, otherwise earlier draft handles would silently
            // observe the newest token.
            draft_tensors.push(
                token
                    .copy()
                    .map_err(|e| candle_to_ocr_inference("GLM-OCR", "save MTP proposal", e))?,
            );
            if draft_tensors.len() == GLM_MTP_DRAFT_TOKENS {
                break;
            }
            let position = text_position_ids(
                target_position + draft_tensors.len() as i64 - 1,
                1,
                &self.device,
            )?;
            let (next_hidden, next_token) = mtp.predict_single(&token, &hidden, &position)?;
            hidden = next_hidden;
            token = next_token
                .reshape((1, 1))
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "MTP proposal shape", e))?;
        }

        let refs: Vec<&Tensor> = draft_tensors.iter().collect();
        Tensor::cat(&refs, 1)
            .and_then(|drafts| drafts.flatten_all())
            .and_then(|drafts| drafts.to_vec1::<u32>())
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "copy MTP proposals", e))
    }

    pub fn decode_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        self.decode_generated_tokens(tokens)
    }

    /// Decode tokens **without** applying GLM-OCR's repetition-collapse
    /// post-process. Use this when the raw token sequence is needed before
    /// any post-processing — for example when feeding output to another
    /// model that operates at token granularity.
    pub fn decode_tokens_raw(&self, tokens: &[u32]) -> Result<String, Error> {
        let decoded = self
            .tokenizer
            .decode(tokens, true)
            .map_err(|e| Error::InvalidInput {
                message: format!("GLM-OCR: tokenizer decode failed: {e}"),
            })?;
        Ok(decoded.trim().to_string())
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    fn decode_generated_tokens(&self, tokens: &[u32]) -> Result<String, Error> {
        // No Rust-specific truncation heuristic — match official GLM-OCR behavior.
        // The official implementation relies on EOS-based stopping only.
        self.decode_tokens_raw(tokens)
    }

    fn prepare_inputs(
        &self,
        input_ids: &[u32],
        image_inputs: &GlmOcrImageInputs,
    ) -> Result<Tensor, Error> {
        let seq_len = input_ids.len();
        let token_ids =
            Tensor::from_vec(input_ids.to_vec(), (1, seq_len), &self.device).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: create input_ids tensor",
                    e,
                )
            })?;
        let mut embeds = self.text.embed(&token_ids)?;

        let image_embeds = self
            .vision
            .forward(&image_inputs.pixel_values, image_inputs.grid_thw)?;
        let image_embeds = image_embeds.to_dtype(self.dtype).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: cast image_embeds",
                e,
            )
        })?;

        let indices: Vec<usize> = input_ids
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| {
                if id == self.image_token_id {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        let image_embeds_len = image_embeds.dim(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: image_embeds dim",
                e,
            )
        })?;
        if indices.len() != image_embeds_len {
            return Err(Error::InvalidInput {
                message: format!(
                    "GLM-OCR: image token count ({}) != image embeds ({})",
                    indices.len(),
                    image_embeds_len
                ),
            });
        }
        if indices.is_empty() {
            return Err(Error::InvalidInput {
                message: "GLM-OCR: no image tokens found in prompt".to_string(),
            });
        }
        let start = indices[0];
        let end = *indices.last().unwrap();
        if end + 1 - start != indices.len() {
            return Err(Error::InvalidInput {
                message: "GLM-OCR: image tokens are not contiguous in prompt".to_string(),
            });
        }

        let hidden_size = embeds.dim(2).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: embed hidden_size",
                e,
            )
        })?;

        let prefix = if start > 0 {
            embeds.narrow(1, 0, start).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: embed prefix narrow",
                    e,
                )
            })?
        } else {
            Tensor::zeros((1, 0, hidden_size), embeds.dtype(), embeds.device()).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: embed prefix zeros",
                    e,
                )
            })?
        };

        let suffix_start = end + 1;
        let suffix = if suffix_start < seq_len {
            embeds
                .narrow(1, suffix_start, seq_len - suffix_start)
                .map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "GLM-OCR: embed suffix narrow",
                        e,
                    )
                })?
        } else {
            Tensor::zeros((1, 0, hidden_size), embeds.dtype(), embeds.device()).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: embed suffix zeros",
                    e,
                )
            })?
        };

        let image_embeds = image_embeds.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: image embeds unsqueeze",
                e,
            )
        })?;
        embeds = Tensor::cat(&[&prefix, &image_embeds, &suffix], 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: embed cat",
                e,
            )
        })?;

        Ok(embeds)
    }

    fn logits_from_hidden(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let hidden = hidden.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: logits unsqueeze",
                e,
            )
        })?;
        let logits = self
            .lm_head
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "lm_head forward", e))?;
        logits.squeeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: logits squeeze",
                e,
            )
        })
    }
}

fn token_tensor(token: u32, device: &Device) -> Result<Tensor, Error> {
    Tensor::new(&[token], device)
        .and_then(|token| token.reshape((1, 1)))
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: create token tensor",
                e,
            )
        })
}

fn text_position_ids(start: i64, len: usize, device: &Device) -> Result<Tensor, Error> {
    let positions: Vec<i64> = (0..len).map(|offset| start + offset as i64).collect();
    let mut data = Vec::with_capacity(3 * len);
    data.extend_from_slice(&positions);
    data.extend_from_slice(&positions);
    data.extend_from_slice(&positions);
    Tensor::from_vec(data, (3, 1, len), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: create text position ids",
            e,
        )
    })
}

fn build_prompt(instruction: &str) -> String {
    format!(
        "[gMASK]<sop><|user|>\n<|begin_of_image|><|image|><|end_of_image|>{instruction}<|assistant|>\n"
    )
}

fn expand_image_tokens(prompt: &str, num_image_tokens: usize) -> Result<String, Error> {
    if num_image_tokens == 0 {
        return Err(Error::InvalidInput {
            message: "GLM-OCR: num_image_tokens is zero".to_string(),
        });
    }
    if !prompt.contains("<|image|>") {
        return Err(Error::InvalidInput {
            message: "GLM-OCR: prompt missing <|image|> token".to_string(),
        });
    }
    let placeholder = "<|placeholder|>";
    let repeated = placeholder.repeat(num_image_tokens);
    let expanded = prompt.replacen("<|image|>", &repeated, 1);
    Ok(expanded.replace(placeholder, "<|image|>"))
}

/// Build position IDs tensor and compute max position value.
///
/// Returns `(position_ids, max_pos)` where `max_pos` is used to compute rope_delta.
fn build_position_ids(
    input_ids: &[u32],
    grid_thw: (usize, usize, usize),
    merge_size: usize,
    image_token_id: u32,
    device: &Device,
) -> Result<(Tensor, i64), Error> {
    let (mut pos_t, mut pos_h, mut pos_w) = (Vec::new(), Vec::new(), Vec::new());
    let mut max_pos: i64 = -1;

    let mut current = input_ids.first().copied().unwrap_or(image_token_id) == image_token_id;
    let mut start = 0usize;
    let mut groups = Vec::new();
    for (i, &id) in input_ids.iter().enumerate() {
        let is_image = id == image_token_id;
        if i == 0 {
            current = is_image;
            continue;
        }
        if is_image != current {
            groups.push((current, start, i));
            start = i;
            current = is_image;
        }
    }
    groups.push((current, start, input_ids.len()));

    let (grid_t, grid_h, grid_w) = grid_thw;
    let llm_grid_h = grid_h / merge_size;
    let llm_grid_w = grid_w / merge_size;
    let llm_grid_t = grid_t;
    let mut seen_image = false;

    let expected_image_tokens = llm_grid_t * llm_grid_h * llm_grid_w;
    for (is_image, s, e) in groups {
        let st_idx = max_pos + 1;
        if is_image {
            if seen_image {
                return Err(Error::InvalidInput {
                    message: "GLM-OCR: multiple image groups in prompt are not supported"
                        .to_string(),
                });
            }
            seen_image = true;
            let group_len = e - s;
            if group_len != expected_image_tokens {
                return Err(Error::InvalidInput {
                    message: format!(
                        "GLM-OCR: image token count ({group_len}) != expected ({expected_image_tokens})"
                    ),
                });
            }
            for t in 0..llm_grid_t {
                for h in 0..llm_grid_h {
                    for w in 0..llm_grid_w {
                        pos_t.push(t as i64 + st_idx);
                        pos_h.push(h as i64 + st_idx);
                        pos_w.push(w as i64 + st_idx);
                    }
                }
            }
            let group_max = (llm_grid_t.max(llm_grid_h).max(llm_grid_w) as i64) - 1 + st_idx;
            max_pos = group_max;
        } else {
            let len = e - s;
            for i in 0..len {
                let v = i as i64 + st_idx;
                pos_t.push(v);
                pos_h.push(v);
                pos_w.push(v);
            }
            max_pos = st_idx + len as i64 - 1;
        }
    }

    let seq_len = input_ids.len();
    if pos_t.len() != seq_len {
        return Err(Error::InvalidInput {
            message: format!(
                "GLM-OCR: position_ids length mismatch: {} vs {}",
                pos_t.len(),
                seq_len
            ),
        });
    }

    let mut data = Vec::with_capacity(3 * seq_len);
    data.extend(pos_t);
    data.extend(pos_h);
    data.extend(pos_w);

    let position_ids = Tensor::from_vec(data, (3, 1, seq_len), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: position_ids tensor",
            e,
        )
    })?;
    Ok((position_ids, max_pos))
}
