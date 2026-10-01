//! Unified layout-first document parser: detect layout (PP-DocLayout v2/v3),
//! then crop and recognize each region with a pluggable backend into structured
//! results. The layout source supplies the regions in reading order.
//!
//! Supported backends include PaddleOCR-VL, HunyuanOCR, GLM-OCR, MinerU2.5,
//! MonkeyOCRv2, OvisOCR2, and MinerU-Diffusion.
//!
//! The `doc_parser` example exposes the layout-first PaddleOCR-VL and GLM-OCR
//! paths. For reference-quality full-page parsing, prefer HunyuanOCR's native
//! page prompt and the MinerU models' native two-step extraction pipeline.

use super::utils::{
    DetectedBox, calculate_overlap_ratio, calculate_projection_overlap_ratio, convert_otsl_to_html,
    crop_margin, filter_overlap_boxes, image::resize_for_mineru, text, truncate_repetitive_content,
};
use crate::crop::crop_bounding_box;
use crate::error::Error;
use crate::geometry::BoundingBox;
use crate::layout::LayoutSource;
use crate::structure::{LayoutElement, LayoutElementType, StructureResult, TableResult, TableType};
use image::RgbImage;
use image::{Rgb, imageops};
use std::sync::Arc;

/// Recognition task for a layout element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionTask {
    /// General OCR/text recognition
    Ocr,
    /// Table structure recognition (outputs HTML)
    Table,
    /// Formula recognition (outputs LaTeX)
    Formula,
    /// Chart recognition
    Chart,
}

/// Trait for recognition backends that can process cropped regions.
pub trait RecognitionBackend {
    /// Generate text/content for a cropped image region.
    ///
    /// # Arguments
    /// * `image` - Cropped region image
    /// * `task` - Recognition task type
    /// * `max_tokens` - Maximum tokens to generate
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error>;

    /// Generate content for multiple cropped regions.
    ///
    /// Backends with native batching override this method. The outer result is
    /// for a batch-level failure; inner results retain per-item decode errors.
    /// The default processes requests individually.
    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        if images.len() != tasks.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "region batch: images count ({}) != tasks count ({})",
                    images.len(),
                    tasks.len()
                ),
            });
        }
        Ok(images
            .into_iter()
            .zip(tasks.iter().copied())
            .map(|(image, task)| self.recognize(image, task, max_tokens))
            .collect())
    }

    /// Stable key for requests whose prepared prompt lengths are equal.
    ///
    /// The default does not subdivide a recognition task, so backends must
    /// still handle padding correctly. Backends whose image-token count depends
    /// on resized geometry can override this so batches need no left padding
    /// and single-token decode can use maskless fused kernels.
    fn recognition_batch_key(&self, _image: &RgbImage, _task: RecognitionTask) -> u64 {
        0
    }

    /// Whether this backend requires post-processing for table output.
    /// Some backends emit OTSL and need conversion; PaddleOCR-VL outputs HTML directly.
    fn needs_table_postprocess(&self) -> bool {
        false
    }

    /// Whether this backend benefits from margin cropping for formulas.
    fn needs_formula_preprocess(&self) -> bool {
        false
    }

    /// Whether this backend output may contain repetitive content.
    fn needs_repetition_truncation(&self) -> bool {
        false
    }
}

/// Configuration for the unified document parser.
#[derive(Debug, Clone)]
pub struct DocParserConfig {
    /// Adds extra padding around each detected region before cropping.
    pub crop_pad_ratio: f32,
    /// Maximum tokens for generation per region.
    pub max_tokens: usize,
    /// Drops header/footer/aside/number regions early to reduce recognition calls.
    pub skip_auxiliary_regions: bool,
    /// Drops PP-DocBlockLayout regions (if present).
    pub skip_region_blocks: bool,
    /// Labels to ignore when converting to markdown.
    pub markdown_ignore_labels: Vec<String>,
    /// Centre captions and tables with inline HTML in markdown output, as the
    /// PaddleOCR-VL reference does.
    pub markdown_pretty: bool,
}

impl Default for DocParserConfig {
    fn default() -> Self {
        Self {
            // OpenOCR's CropByBoxes does not add padding around regions.
            crop_pad_ratio: 0.0,
            max_tokens: 4096,
            skip_auxiliary_regions: true,
            skip_region_blocks: true,
            markdown_ignore_labels: super::utils::default_markdown_ignore_labels(),
            markdown_pretty: true,
        }
    }
}

/// Unified document parser combining layout detection with recognition backend.
pub struct DocParser<'a, B: RecognitionBackend> {
    backend: &'a B,
    config: DocParserConfig,
    region_batch_size: usize,
}

impl<'a, B: RecognitionBackend> DocParser<'a, B> {
    /// Create a new document parser with the given backend.
    pub fn new(backend: &'a B) -> Self {
        Self {
            backend,
            config: DocParserConfig::default(),
            region_batch_size: 1,
        }
    }

    /// Create a new document parser with custom configuration.
    pub fn with_config(backend: &'a B, config: DocParserConfig) -> Self {
        Self {
            backend,
            config,
            region_batch_size: 1,
        }
    }

    /// Set the maximum number of same-task regions sent to the recognition
    /// backend at once. A value of zero is treated as one.
    ///
    /// The default is one so existing callers retain their memory use and
    /// output behavior. PaddleOCR-VL, HunyuanOCR, and MinerU perform native
    /// batched inference; the other backends use a correct sequential path.
    /// PaddleOCR-VL and HunyuanOCR additionally group equal image-token counts,
    /// which keeps the common Metal decode path padding-free. Any padded native
    /// batch remains correct but uses the slower masked attention path.
    pub fn with_region_batch_size(mut self, size: usize) -> Self {
        self.region_batch_size = size.max(1);
        self
    }

    /// Return the configured cropped-region batch size.
    pub fn region_batch_size(&self) -> usize {
        self.region_batch_size
    }

    /// Returns a reference to the parser's configuration.
    pub fn config(&self) -> &DocParserConfig {
        &self.config
    }

    /// Parse a document image using layout-first pipeline.
    ///
    /// `layout` is any [`LayoutSource`]: [`PpDocLayout`](crate::PpDocLayout),
    /// which runs on Candle like the rest of this crate, or a detector of your
    /// own. Its regions are used in the order returned.
    pub fn parse<L: LayoutSource + ?Sized>(
        &self,
        layout: &L,
        image: RgbImage,
    ) -> Result<StructureResult, Error> {
        self.parse_with_path(layout, "<memory>", 0, image)
    }

    /// Parse a document image without layout detection (single full-image OCR).
    ///
    /// Use this for end-to-end models that handle layout internally.
    /// For models requiring separate layout detection, use [`parse`](Self::parse) instead.
    pub fn parse_without_layout(&self, image: RgbImage) -> Result<StructureResult, Error> {
        self.recognize_full_image("<memory>".into(), 0, image)
    }

    /// Parse a document image with source path information.
    pub fn parse_with_path<L: LayoutSource + ?Sized>(
        &self,
        layout: &L,
        input_path: impl Into<Arc<str>>,
        index: usize,
        image: RgbImage,
    ) -> Result<StructureResult, Error> {
        let input_path: Arc<str> = input_path.into();
        let (page_w, page_h) = (image.width() as f32, image.height() as f32);

        // Step 1: Layout detection
        let layout_result = layout.detect(&image)?;
        let detected = layout_result.elements;

        // OpenOCR applies an additional overlap filter (overlap_ratio > 0.7, mode=small)
        // after PP-DocLayout (v2/v3) to remove redundant boxes.
        let detected: Vec<DetectedBox> = detected
            .into_iter()
            .map(|e| DetectedBox {
                bbox: e.bbox,
                label: e.element_type,
                score: e.score,
            })
            .collect();
        let detected = filter_overlap_boxes(detected, 0.7);

        // If no layout elements detected, run OCR on the whole image
        if detected.is_empty() {
            return self.recognize_full_image(input_path, index, image);
        }

        // Step 2: Filter and prepare elements
        let mut elements: Vec<LayoutElement> = Vec::with_capacity(detected.len());
        for element in detected {
            let element_type = LayoutElementType::from_label(&element.label);
            if self.config.skip_region_blocks && element_type == LayoutElementType::Region {
                continue;
            }
            if self.config.skip_auxiliary_regions && is_auxiliary_element(element_type) {
                continue;
            }

            elements.push(
                LayoutElement::new(element.bbox, element_type, element.score)
                    .with_label(element.label),
            );
        }

        if elements.is_empty() {
            return self.recognize_full_image(input_path, index, image);
        }

        // Step 3: Number the elements. A `LayoutSource` hands them over in
        // reading order, so there is nothing to sort here.
        let mut sorted_elements = elements;
        assign_order_indices(&mut sorted_elements);

        // Step 4: Recognize each element (with OpenOCR-style block merging)
        //
        // OpenOCR merges adjacent "text" blocks before recognition by vertically stacking crops.
        // This can improve recognition quality for fragmented detections.
        let merge_groups = compute_openocr_merge_groups(&sorted_elements);
        let mut group_first_for_index: Vec<Option<usize>> = vec![None; sorted_elements.len()];
        let mut group_by_first: std::collections::HashMap<usize, MergeGroup> =
            std::collections::HashMap::new();
        for group in merge_groups {
            let first = group.indices[0];
            for &idx in &group.indices {
                if idx < group_first_for_index.len() {
                    group_first_for_index[idx] = Some(first);
                }
            }
            group_by_first.insert(first, group);
        }

        let element_bboxes: Vec<BoundingBox> =
            sorted_elements.iter().map(|el| el.bbox.clone()).collect();

        struct RecognitionJob {
            element_index: usize,
            image: RgbImage,
            task: RecognitionTask,
        }

        let mut jobs_by_batch: std::collections::BTreeMap<(usize, u64), Vec<RecognitionJob>> =
            std::collections::BTreeMap::new();
        let max_pending_jobs = self.region_batch_size.saturating_mul(8).max(8);
        let mut pending_jobs = 0usize;
        let mut tasks_by_element = vec![None; sorted_elements.len()];
        let mut generated_by_element: Vec<Option<Result<String, Error>>> =
            std::iter::repeat_with(|| None)
                .take(sorted_elements.len())
                .collect();
        let flush_batch = |jobs: &mut Vec<RecognitionJob>,
                           generated: &mut [Option<Result<String, Error>>]|
         -> Result<(), Error> {
            let batch = std::mem::take(jobs);
            let element_indices: Vec<usize> = batch.iter().map(|job| job.element_index).collect();
            let tasks: Vec<RecognitionTask> = batch.iter().map(|job| job.task).collect();
            let images: Vec<RgbImage> = batch.into_iter().map(|job| job.image).collect();
            let results = self
                .backend
                .recognize_batch(images, &tasks, self.config.max_tokens)?;
            if results.len() != element_indices.len() {
                return Err(Error::InvalidInput {
                    message: format!(
                        "recognition backend returned {} results for {} inputs",
                        results.len(),
                        element_indices.len()
                    ),
                });
            }

            for (element_index, result) in element_indices.into_iter().zip(results) {
                generated[element_index] = Some(result);
            }
            Ok(())
        };
        let mut merged_by_first: std::collections::HashMap<usize, bool> =
            std::collections::HashMap::new();
        for (idx, element) in sorted_elements.iter_mut().enumerate() {
            if let Some(first) = group_first_for_index.get(idx).and_then(|v| *v)
                && first != idx
            {
                // OpenOCR only drops non-first blocks *when a merge actually happens*.
                // When the group is not merged (aspect_ratio >= 3), each block is recognized separately.
                if merged_by_first.get(&first).copied().unwrap_or(false) {
                    continue;
                }
            }

            // Determine task for element
            let Some(task) = task_for_element_type(element.element_type) else {
                continue;
            };

            let group = group_by_first.get(&idx);
            let mut cropped = if let Some(group) = group {
                // Merge all crops in the group into a single stacked image.
                let mut crops: Vec<RgbImage> = Vec::with_capacity(group.indices.len());
                for &g_idx in &group.indices {
                    let Some(bbox) = element_bboxes.get(g_idx) else {
                        continue;
                    };
                    let crop_bbox = if self.config.crop_pad_ratio > 0.0 {
                        pad_bbox(bbox, page_w, page_h, self.config.crop_pad_ratio)
                    } else {
                        bbox.clone()
                    };
                    let crop = match crop_bounding_box(&image, &crop_bbox) {
                        Ok(crop) => crop,
                        Err(_) => continue,
                    };
                    crops.push(crop);
                }
                if crops.is_empty() {
                    continue;
                }
                // Decide whether to merge based on aspect ratio (OpenOCR: skip merge if h/w >= 3).
                let max_w = crops.iter().map(|c| c.width()).max().unwrap_or(0);
                let sum_h: u32 = crops.iter().map(|c| c.height()).sum();
                let aspect_ratio = if max_w == 0 {
                    f32::INFINITY
                } else {
                    sum_h as f32 / max_w as f32
                };
                if aspect_ratio >= 3.0 || crops.len() == 1 {
                    merged_by_first.insert(idx, false);
                    // Fallback: use this element's own crop only.
                    let crop_bbox = if self.config.crop_pad_ratio > 0.0 {
                        pad_bbox(&element.bbox, page_w, page_h, self.config.crop_pad_ratio)
                    } else {
                        element.bbox.clone()
                    };
                    match crop_bounding_box(&image, &crop_bbox) {
                        Ok(crop) => crop,
                        Err(_) => continue,
                    }
                } else {
                    merged_by_first.insert(idx, true);
                    merge_images_vertically(&crops, &group.aligns)
                }
            } else {
                let crop_bbox = if self.config.crop_pad_ratio > 0.0 {
                    pad_bbox(&element.bbox, page_w, page_h, self.config.crop_pad_ratio)
                } else {
                    element.bbox.clone()
                };
                match crop_bounding_box(&image, &crop_bbox) {
                    Ok(cropped) => cropped,
                    Err(_) => continue,
                }
            };

            // Apply preprocessing if needed
            if task == RecognitionTask::Formula && self.backend.needs_formula_preprocess() {
                cropped = crop_margin(&cropped);
            }

            tasks_by_element[idx] = Some(task);
            let batch_key = (
                recognition_task_index(task),
                self.backend.recognition_batch_key(&cropped, task),
            );
            {
                let jobs = jobs_by_batch.entry(batch_key).or_default();
                jobs.push(RecognitionJob {
                    element_index: idx,
                    image: cropped,
                    task,
                });
                pending_jobs += 1;
                if jobs.len() == self.region_batch_size {
                    flush_batch(jobs, &mut generated_by_element)?;
                    pending_jobs -= self.region_batch_size;
                }
            }

            // Exact token-count bucketing can produce many sparse queues on a
            // page with varied region sizes. Bound retained crop memory and
            // flush the fullest queue when the cap is reached.
            if pending_jobs >= max_pending_jobs
                && let Some(key) = jobs_by_batch
                    .iter()
                    .filter(|(_, jobs)| !jobs.is_empty())
                    .max_by_key(|(_, jobs)| jobs.len())
                    .map(|(key, _)| *key)
            {
                let jobs = jobs_by_batch
                    .get_mut(&key)
                    .expect("selected recognition queue must exist");
                pending_jobs -= jobs.len();
                flush_batch(jobs, &mut generated_by_element)?;
            }
        }

        // Native VLM batches are most efficient when prompts/tasks are
        // homogeneous. Full queues and the global pending-job cap keep retained
        // crop memory bounded rather than proportional to all page regions.
        for jobs in jobs_by_batch.values_mut() {
            if !jobs.is_empty() {
                flush_batch(jobs, &mut generated_by_element)?;
            }
        }

        let mut tables: Vec<TableResult> = Vec::new();
        for (idx, generated) in generated_by_element.into_iter().enumerate() {
            let Some(generated) = generated else {
                continue;
            };
            let Some(task) = tasks_by_element[idx] else {
                continue;
            };
            let element = &mut sorted_elements[idx];
            let mut generated = generated?;
            if generated.trim().is_empty() {
                continue;
            }

            // Apply repetition truncation if needed
            if self.backend.needs_repetition_truncation() {
                generated = truncate_repetitive_content(&generated, 10, 10, 10);
            }

            // Apply post-processing based on task.
            //
            // Note: Table blocks should remain HTML/OTSL-derived HTML; do not run the text normalizer
            // on table markup.
            let processed = if task == RecognitionTask::Table {
                if self.backend.needs_table_postprocess() {
                    convert_otsl_to_html(&generated)
                } else {
                    generated.trim().to_string()
                }
            } else if task == RecognitionTask::Formula {
                text::format_formula(&generated)
            } else {
                text::format_text(&generated)
            };

            if element.element_type == LayoutElementType::Table {
                tables.push(
                    TableResult::new(element.bbox.clone(), TableType::Unknown)
                        .with_html_structure(processed.clone()),
                );
            }

            element.text = Some(processed);
        }

        Ok(StructureResult::new(input_path, index)
            .with_layout_elements(sorted_elements)
            .with_tables(tables))
    }

    /// Parse a document and convert to markdown.
    ///
    /// Uses [`DocParserConfig::markdown_ignore_labels`] and
    /// [`DocParserConfig::markdown_pretty`].
    pub fn parse_to_markdown<L: LayoutSource + ?Sized>(
        &self,
        layout: &L,
        image: RgbImage,
    ) -> Result<String, Error> {
        let result = self.parse(layout, image)?;
        Ok(super::utils::to_markdown(
            &result.layout_elements,
            &self.config.markdown_ignore_labels,
            self.config.markdown_pretty,
        ))
    }

    fn recognize_full_image(
        &self,
        input_path: Arc<str>,
        index: usize,
        image: RgbImage,
    ) -> Result<StructureResult, Error> {
        let (page_w, page_h) = (image.width() as f32, image.height() as f32);
        let text = self
            .backend
            .recognize(image, RecognitionTask::Ocr, self.config.max_tokens)?;
        let element = LayoutElement::new(
            BoundingBox::from_coords(0.0, 0.0, page_w, page_h),
            LayoutElementType::Text,
            1.0,
        )
        .with_label("text")
        .with_text(text.trim());
        Ok(StructureResult::new(input_path, index).with_layout_elements(vec![element]))
    }
}

use super::glmocr::GlmOcr;
use super::hunyuanocr::HunyuanOcr;
use super::mineru::MinerU;
use super::paddleocr_vl::{PaddleOcrVl, PaddleOcrVlTask};

impl RecognitionBackend for PaddleOcrVl {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        let vl_task = match task {
            RecognitionTask::Ocr => PaddleOcrVlTask::Ocr,
            RecognitionTask::Table => PaddleOcrVlTask::Table,
            RecognitionTask::Formula => PaddleOcrVlTask::Formula,
            RecognitionTask::Chart => PaddleOcrVlTask::Chart,
        };

        // PaddleOCR-VL's reference pipeline truncates repetitive tails on the *raw* model output,
        // before per-task postprocessing (e.g., table-token conversion).
        let results = self.generate_with_raw(&[image], &[vl_task], max_tokens)?;
        let (raw, _) = results.into_iter().next().ok_or(Error::InvalidInput {
            message: "PaddleOCR-VL: no result returned".to_string(),
        })??;
        let raw = truncate_repetitive_content(&raw, 10, 10, 10);
        Ok(vl_task.postprocess(raw))
    }

    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        let vl_tasks: Vec<PaddleOcrVlTask> = tasks
            .iter()
            .map(|task| match task {
                RecognitionTask::Ocr => PaddleOcrVlTask::Ocr,
                RecognitionTask::Table => PaddleOcrVlTask::Table,
                RecognitionTask::Formula => PaddleOcrVlTask::Formula,
                RecognitionTask::Chart => PaddleOcrVlTask::Chart,
            })
            .collect();

        Ok(self
            .generate_with_raw(&images, &vl_tasks, max_tokens)?
            .into_iter()
            .zip(vl_tasks)
            .map(|(result, task)| {
                result.map(|(raw, _)| {
                    let raw = truncate_repetitive_content(&raw, 10, 10, 10);
                    task.postprocess(raw)
                })
            })
            .collect())
    }

    fn recognition_batch_key(&self, image: &RgbImage, _task: RecognitionTask) -> u64 {
        PaddleOcrVl::recognition_batch_key(self, image).unwrap_or(0)
    }

    fn needs_table_postprocess(&self) -> bool {
        false // PaddleOCR-VL outputs HTML directly
    }

    fn needs_formula_preprocess(&self) -> bool {
        true // Match PaddleOCR-VL pipeline: crop formula margins before recognition
    }

    fn needs_repetition_truncation(&self) -> bool {
        false // Handled inside `recognize()` (must run before per-task postprocess)
    }
}

impl RecognitionBackend for HunyuanOcr {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        let prompt = match task {
            RecognitionTask::Ocr => {
                "Detect and recognize text in the image, and output the text coordinates in a formatted manner."
            }
            RecognitionTask::Table => "Parse the table in the image into HTML.",
            RecognitionTask::Formula => {
                "Identify the formula in the image and represent it using LaTeX format."
            }
            RecognitionTask::Chart => {
                "Parse the chart in the image; use Mermaid format for flowcharts and Markdown for other charts."
            }
        };
        let out = self
            .generate(&[image], &[prompt], max_tokens)?
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                Err(Error::InvalidInput {
                    message: "HunyuanOCR: no result returned".to_string(),
                })
            })?;
        Ok(truncate_repetitive_content(&out, 10, 10, 10)
            .trim()
            .to_string())
    }

    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        let prompts: Vec<&str> = tasks
            .iter()
            .map(|task| match task {
                RecognitionTask::Ocr => {
                    "Detect and recognize text in the image, and output the text coordinates in a formatted manner."
                }
                RecognitionTask::Table => "Parse the table in the image into HTML.",
                RecognitionTask::Formula => {
                    "Identify the formula in the image and represent it using LaTeX format."
                }
                RecognitionTask::Chart => {
                    "Parse the chart in the image; use Mermaid format for flowcharts and Markdown for other charts."
                }
            })
            .collect();

        Ok(self
            .generate(&images, &prompts, max_tokens)?
            .into_iter()
            .map(|result| {
                result.map(|out| {
                    truncate_repetitive_content(&out, 10, 10, 10)
                        .trim()
                        .to_string()
                })
            })
            .collect())
    }

    fn recognition_batch_key(&self, image: &RgbImage, _task: RecognitionTask) -> u64 {
        HunyuanOcr::recognition_batch_key(self, image).unwrap_or(0)
    }

    fn needs_table_postprocess(&self) -> bool {
        false
    }

    fn needs_formula_preprocess(&self) -> bool {
        false
    }

    fn needs_repetition_truncation(&self) -> bool {
        false // handled inside `recognize()`
    }
}

impl RecognitionBackend for GlmOcr {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        let prompt = match task {
            RecognitionTask::Ocr => "Text Recognition:",
            RecognitionTask::Table => "Table Recognition:",
            RecognitionTask::Formula => "Formula Recognition:",
            RecognitionTask::Chart => "Text Recognition:",
        };
        let out = self
            .generate(&[image], &[prompt], max_tokens)?
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                Err(Error::InvalidInput {
                    message: "GLM-OCR: no result returned".to_string(),
                })
            })?;
        Ok(truncate_repetitive_content(&out, 10, 10, 10)
            .trim()
            .to_string())
    }

    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        let prompts: Vec<&str> = tasks
            .iter()
            .map(|task| match task {
                RecognitionTask::Ocr => "Text Recognition:",
                RecognitionTask::Table => "Table Recognition:",
                RecognitionTask::Formula => "Formula Recognition:",
                RecognitionTask::Chart => "Text Recognition:",
            })
            .collect();

        Ok(self
            .generate(&images, &prompts, max_tokens)?
            .into_iter()
            .map(|result| {
                result.map(|out| {
                    truncate_repetitive_content(&out, 10, 10, 10)
                        .trim()
                        .to_string()
                })
            })
            .collect())
    }

    fn needs_table_postprocess(&self) -> bool {
        false
    }

    fn needs_formula_preprocess(&self) -> bool {
        false
    }

    fn needs_repetition_truncation(&self) -> bool {
        false // handled inside `recognize()`
    }
}

impl RecognitionBackend for MinerU {
    fn recognize(
        &self,
        image: RgbImage,
        task: RecognitionTask,
        max_tokens: usize,
    ) -> Result<String, Error> {
        let prompt = match task {
            RecognitionTask::Ocr => "\nText Recognition:",
            RecognitionTask::Table => "\nTable Recognition:",
            RecognitionTask::Formula => "\nFormula Recognition:",
            RecognitionTask::Chart => "\nDocument Parsing:",
        };
        // MinerU requires images with minimum edge >= 28 (factor constraint)
        // Preprocess small or extremely aspect-ratioed images
        let processed = resize_for_mineru(&image, 28, 50.0);
        let out = self
            .generate(&[processed], &[prompt], max_tokens)?
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                Err(Error::InvalidInput {
                    message: "MinerU2.5: no result returned".to_string(),
                })
            })?;
        Ok(truncate_repetitive_content(&out, 10, 10, 10)
            .trim()
            .to_string())
    }

    fn recognize_batch(
        &self,
        images: Vec<RgbImage>,
        tasks: &[RecognitionTask],
        max_tokens: usize,
    ) -> crate::error::BatchResult<String> {
        let processed: Vec<RgbImage> = images
            .iter()
            .map(|image| resize_for_mineru(image, 28, 50.0))
            .collect();
        let prompts: Vec<&str> = tasks
            .iter()
            .map(|task| match task {
                RecognitionTask::Ocr => "\nText Recognition:",
                RecognitionTask::Table => "\nTable Recognition:",
                RecognitionTask::Formula => "\nFormula Recognition:",
                RecognitionTask::Chart => "\nDocument Parsing:",
            })
            .collect();

        Ok(self
            .generate(&processed, &prompts, max_tokens)?
            .into_iter()
            .map(|result| {
                result.map(|out| {
                    truncate_repetitive_content(&out, 10, 10, 10)
                        .trim()
                        .to_string()
                })
            })
            .collect())
    }

    fn needs_table_postprocess(&self) -> bool {
        true
    }

    fn needs_formula_preprocess(&self) -> bool {
        false
    }

    fn needs_repetition_truncation(&self) -> bool {
        false // handled inside `recognize()`
    }
}

fn is_auxiliary_element(element_type: LayoutElementType) -> bool {
    matches!(
        element_type,
        LayoutElementType::Number
            | LayoutElementType::Footnote
            | LayoutElementType::Header
            | LayoutElementType::HeaderImage
            | LayoutElementType::Footer
            | LayoutElementType::FooterImage
            | LayoutElementType::AsideText
    )
}

const fn recognition_task_index(task: RecognitionTask) -> usize {
    match task {
        RecognitionTask::Ocr => 0,
        RecognitionTask::Table => 1,
        RecognitionTask::Formula => 2,
        RecognitionTask::Chart => 3,
    }
}

fn task_for_element_type(element_type: LayoutElementType) -> Option<RecognitionTask> {
    match element_type {
        LayoutElementType::Table => Some(RecognitionTask::Table),
        LayoutElementType::Chart => Some(RecognitionTask::Chart),
        LayoutElementType::Formula => Some(RecognitionTask::Formula),
        LayoutElementType::FormulaNumber => Some(RecognitionTask::Ocr),
        // Skip pure visual regions
        LayoutElementType::Image
        | LayoutElementType::HeaderImage
        | LayoutElementType::FooterImage
        | LayoutElementType::Seal => None,
        _ => Some(RecognitionTask::Ocr),
    }
}

fn pad_bbox(bbox: &BoundingBox, page_w: f32, page_h: f32, pad_ratio: f32) -> BoundingBox {
    let x1 = bbox.x_min();
    let y1 = bbox.y_min();
    let x2 = bbox.x_max();
    let y2 = bbox.y_max();
    let w = (x2 - x1).max(1.0);
    let h = (y2 - y1).max(1.0);
    let pad_x = w * pad_ratio;
    let pad_y = h * pad_ratio;

    BoundingBox::from_coords(
        (x1 - pad_x).max(0.0),
        (y1 - pad_y).max(0.0),
        (x2 + pad_x).min(page_w),
        (y2 + pad_y).min(page_h),
    )
}

fn assign_order_indices(elements: &mut [LayoutElement]) {
    let mut order_index = 1u32;
    for element in elements.iter_mut() {
        if should_have_order_index(element.element_type) {
            element.order_index = Some(order_index);
            order_index += 1;
        }
    }
}

fn should_have_order_index(element_type: LayoutElementType) -> bool {
    matches!(
        element_type,
        LayoutElementType::Text
            | LayoutElementType::Content
            | LayoutElementType::Abstract
            | LayoutElementType::DocTitle
            | LayoutElementType::ParagraphTitle
            | LayoutElementType::Table
            | LayoutElementType::Image
            | LayoutElementType::Chart
            | LayoutElementType::Formula
            | LayoutElementType::Seal
            | LayoutElementType::Reference
            | LayoutElementType::ReferenceContent
            | LayoutElementType::List
            | LayoutElementType::FigureTitle
            | LayoutElementType::TableTitle
            | LayoutElementType::ChartTitle
            | LayoutElementType::FigureTableChartTitle
    )
}

/// Document parser using PaddleOCR-VL backend.
pub type PaddleOcrVlDocParser<'a> = DocParser<'a, PaddleOcrVl>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeAlign {
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone)]
struct MergeGroup {
    indices: Vec<usize>,
    aligns: Vec<MergeAlign>,
}

fn merge_images_vertically(images: &[RgbImage], aligns: &[MergeAlign]) -> RgbImage {
    if images.is_empty() {
        return RgbImage::new(0, 0);
    }
    if images.len() == 1 {
        return images[0].clone();
    }

    let mut merged = images[0].clone();
    for (i, img2) in images.iter().enumerate().skip(1) {
        let align = aligns.get(i - 1).copied().unwrap_or(MergeAlign::Center);
        let w = merged.width().max(img2.width());
        let h = merged.height() + img2.height();
        let mut new_img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));

        let (x1, x2) = match align {
            MergeAlign::Center => (
                ((w - merged.width()) / 2) as i64,
                ((w - img2.width()) / 2) as i64,
            ),
            MergeAlign::Right => ((w - merged.width()) as i64, (w - img2.width()) as i64),
            MergeAlign::Left => (0, 0),
        };

        imageops::overlay(&mut new_img, &merged, x1, 0);
        imageops::overlay(&mut new_img, img2, x2, merged.height() as i64);
        merged = new_img;
    }
    merged
}

fn compute_openocr_merge_groups(elements: &[LayoutElement]) -> Vec<MergeGroup> {
    // Match OpenOCR: merge only non-image/table blocks; actual merge conditions are label == "text".
    const NON_MERGE_LABELS: [&str; 6] = [
        "image",
        "header_image",
        "footer_image",
        "seal",
        "table",
        "chart",
    ];

    let mut blocks_to_merge: Vec<usize> = Vec::new();
    for (idx, element) in elements.iter().enumerate() {
        let label = element.label.as_deref().unwrap_or("");
        if NON_MERGE_LABELS.contains(&label) {
            continue;
        }
        blocks_to_merge.push(idx);
    }

    if blocks_to_merge.len() < 2 {
        return Vec::new();
    }

    let mut merged_groups: Vec<MergeGroup> = Vec::new();
    let mut current_indices: Vec<usize> = Vec::new();
    let mut current_aligns: Vec<MergeAlign> = Vec::new();

    fn is_aligned(a1: f32, a2: f32) -> bool {
        (a1 - a2).abs() <= 5.0
    }

    fn rect_xyxy(bbox: &BoundingBox) -> (f32, f32, f32, f32) {
        (bbox.x_min(), bbox.y_min(), bbox.x_max(), bbox.y_max())
    }

    fn get_alignment(curr: (f32, f32, f32, f32), prev: (f32, f32, f32, f32)) -> MergeAlign {
        if is_aligned(curr.0, prev.0) {
            MergeAlign::Left
        } else if is_aligned(curr.2, prev.2) {
            MergeAlign::Right
        } else {
            MergeAlign::Center
        }
    }

    fn overlapwith_other_box(
        block_idx: usize,
        prev_idx: usize,
        elements: &[LayoutElement],
    ) -> bool {
        let prev_bbox = &elements[prev_idx].bbox;
        let block_bbox = &elements[block_idx].bbox;

        let (px1, py1, px2, py2) = rect_xyxy(prev_bbox);
        let (bx1, by1, bx2, by2) = rect_xyxy(block_bbox);

        let min_box =
            BoundingBox::from_coords(px1.min(bx1), py1.min(by1), px2.max(bx2), py2.max(by2));

        for (idx, other) in elements.iter().enumerate() {
            if idx == block_idx || idx == prev_idx {
                continue;
            }
            if calculate_overlap_ratio(&min_box, &other.bbox, "union") > 0.0 {
                return true;
            }
        }
        false
    }

    for (i, &idx) in blocks_to_merge.iter().enumerate() {
        if current_indices.is_empty() {
            current_indices.push(idx);
            continue;
        }

        let prev_idx = blocks_to_merge[i - 1];
        let prev_label = elements[prev_idx].label.as_deref().unwrap_or("");
        let curr_label = elements[idx].label.as_deref().unwrap_or("");

        let prev_rect = rect_xyxy(&elements[prev_idx].bbox);
        let curr_rect = rect_xyxy(&elements[idx].bbox);

        let iou_h = calculate_projection_overlap_ratio(
            &elements[idx].bbox,
            &elements[prev_idx].bbox,
            "horizontal",
            "union",
        );

        let prev_w = (prev_rect.2 - prev_rect.0).max(0.0);
        let curr_w = (curr_rect.2 - curr_rect.0).max(0.0);
        let prev_h = (prev_rect.3 - prev_rect.1).max(0.0);
        let curr_h = (curr_rect.3 - curr_rect.1).max(0.0);

        let is_cross = iou_h == 0.0
            && curr_label == "text"
            && curr_label == prev_label
            && curr_rect.0 > prev_rect.2
            && curr_rect.1 < prev_rect.3
            && (curr_rect.0 - prev_rect.2) < prev_w.max(curr_w) * 0.3;

        let left_aligned = is_aligned(curr_rect.0, prev_rect.0);
        let right_aligned = is_aligned(curr_rect.2, prev_rect.2);

        let is_updown_align = iou_h > 0.0
            && curr_label == "text"
            && curr_label == prev_label
            && curr_rect.3 >= prev_rect.1
            && (curr_rect.1 - prev_rect.3).abs() < prev_h.max(curr_h) * 0.5
            && (left_aligned ^ right_aligned)
            && overlapwith_other_box(idx, prev_idx, elements);

        let align_mode = if is_cross {
            Some(MergeAlign::Center)
        } else if is_updown_align {
            Some(get_alignment(curr_rect, prev_rect))
        } else {
            None
        };

        if is_cross || is_updown_align {
            current_indices.push(idx);
            if let Some(a) = align_mode {
                current_aligns.push(a);
            }
        } else {
            merged_groups.push(MergeGroup {
                indices: std::mem::take(&mut current_indices),
                aligns: std::mem::take(&mut current_aligns),
            });
            current_indices.push(idx);
        }
    }

    if !current_indices.is_empty() {
        merged_groups.push(MergeGroup {
            indices: current_indices,
            aligns: current_aligns,
        });
    }

    merged_groups
        .into_iter()
        .filter(|g| g.indices.len() > 1 && g.aligns.len() + 1 == g.indices.len())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutDetectionElement;
    use crate::layout::StaticLayout;
    use std::cell::{Cell, RefCell};

    /// Records the crops it is handed and echoes back a per-call marker.
    struct RecordingBackend {
        crops: RefCell<Vec<(u32, u32)>>,
    }

    impl RecognitionBackend for RecordingBackend {
        fn recognize(
            &self,
            image: RgbImage,
            _task: RecognitionTask,
            _max_tokens: usize,
        ) -> Result<String, Error> {
            let mut crops = self.crops.borrow_mut();
            crops.push((image.width(), image.height()));
            Ok(format!("region {}", crops.len()))
        }
    }

    struct BatchRecordingBackend {
        batch_sizes: RefCell<Vec<usize>>,
    }

    impl RecognitionBackend for BatchRecordingBackend {
        fn recognize(
            &self,
            _image: RgbImage,
            _task: RecognitionTask,
            _max_tokens: usize,
        ) -> Result<String, Error> {
            panic!("native batch hook should be used")
        }

        fn recognize_batch(
            &self,
            images: Vec<RgbImage>,
            tasks: &[RecognitionTask],
            _max_tokens: usize,
        ) -> crate::error::BatchResult<String> {
            assert_eq!(images.len(), tasks.len());
            self.batch_sizes.borrow_mut().push(images.len());
            Ok(images
                .into_iter()
                .map(|_| Ok("batched region".to_string()))
                .collect())
        }

        fn recognition_batch_key(&self, image: &RgbImage, _task: RecognitionTask) -> u64 {
            image.width() as u64
        }
    }

    struct SparseBatchKeyBackend {
        keys_seen: Cell<usize>,
        first_flush_at: Cell<Option<usize>>,
    }

    struct FailingBatchBackend;

    impl RecognitionBackend for FailingBatchBackend {
        fn recognize(
            &self,
            _image: RgbImage,
            _task: RecognitionTask,
            _max_tokens: usize,
        ) -> Result<String, Error> {
            panic!("native batch hook should be used")
        }

        fn recognize_batch(
            &self,
            _images: Vec<RgbImage>,
            _tasks: &[RecognitionTask],
            _max_tokens: usize,
        ) -> crate::error::BatchResult<String> {
            Err(Error::Config {
                message: "native batch failed".to_string(),
            })
        }
    }

    impl RecognitionBackend for SparseBatchKeyBackend {
        fn recognize(
            &self,
            _image: RgbImage,
            _task: RecognitionTask,
            _max_tokens: usize,
        ) -> Result<String, Error> {
            panic!("native batch hook should be used")
        }

        fn recognize_batch(
            &self,
            images: Vec<RgbImage>,
            tasks: &[RecognitionTask],
            _max_tokens: usize,
        ) -> crate::error::BatchResult<String> {
            assert_eq!(images.len(), tasks.len());
            if self.first_flush_at.get().is_none() {
                self.first_flush_at.set(Some(self.keys_seen.get()));
            }
            Ok(images
                .into_iter()
                .map(|_| Ok("bounded region".to_string()))
                .collect())
        }

        fn recognition_batch_key(&self, image: &RgbImage, _task: RecognitionTask) -> u64 {
            self.keys_seen.set(self.keys_seen.get() + 1);
            image.width() as u64
        }
    }

    fn element(label: &str, x1: f32, y1: f32, x2: f32, y2: f32) -> LayoutDetectionElement {
        LayoutDetectionElement {
            bbox: BoundingBox::from_coords(x1, y1, x2, y2),
            element_type: label.to_string(),
            score: 0.9,
        }
    }

    /// The parser must run end to end on a caller-supplied `LayoutSource`,
    /// which is the path that needs no ONNX Runtime.
    #[test]
    fn parses_with_a_static_layout_source() {
        let backend = RecordingBackend {
            crops: RefCell::new(Vec::new()),
        };
        let parser = DocParser::new(&backend);
        let layout = StaticLayout::new(vec![
            element("doc_title", 10.0, 10.0, 190.0, 40.0),
            element("text", 10.0, 60.0, 190.0, 140.0),
        ]);

        let result = parser
            .parse(&layout, RgbImage::new(200, 200))
            .expect("parse succeeds");

        assert_eq!(result.layout_elements.len(), 2);
        assert_eq!(backend.crops.borrow().len(), 2);
        assert_eq!(
            result.layout_elements[0].element_type,
            LayoutElementType::DocTitle
        );
    }

    #[test]
    fn batches_same_task_regions_and_preserves_element_results() {
        let backend = BatchRecordingBackend {
            batch_sizes: RefCell::new(Vec::new()),
        };
        let parser = DocParser::new(&backend).with_region_batch_size(2);
        let layout = StaticLayout::new(vec![
            element("doc_title", 10.0, 0.0, 190.0, 20.0),
            element("paragraph_title", 10.0, 30.0, 190.0, 50.0),
            element("content", 10.0, 60.0, 190.0, 80.0),
            element("abstract", 10.0, 90.0, 190.0, 110.0),
            element("list", 10.0, 120.0, 190.0, 140.0),
        ]);

        let result = parser
            .parse(&layout, RgbImage::new(200, 160))
            .expect("parse succeeds");

        assert_eq!(*backend.batch_sizes.borrow(), vec![2, 2, 1]);
        assert_eq!(result.layout_elements.len(), 5);
        assert!(
            result
                .layout_elements
                .iter()
                .all(|element| element.text.as_deref() == Some("batched region"))
        );
    }

    #[test]
    fn separates_same_task_regions_with_different_batch_keys() {
        let backend = BatchRecordingBackend {
            batch_sizes: RefCell::new(Vec::new()),
        };
        let parser = DocParser::new(&backend).with_region_batch_size(2);
        let layout = StaticLayout::new(vec![
            element("doc_title", 0.0, 0.0, 100.0, 20.0),
            element("paragraph_title", 0.0, 30.0, 120.0, 50.0),
            element("content", 0.0, 60.0, 100.0, 80.0),
            element("abstract", 0.0, 90.0, 120.0, 110.0),
        ]);

        let result = parser
            .parse(&layout, RgbImage::new(140, 120))
            .expect("parse succeeds");

        assert_eq!(*backend.batch_sizes.borrow(), vec![2, 2]);
        assert_eq!(result.layout_elements.len(), 4);
    }

    #[test]
    fn sparse_batch_keys_flush_before_retaining_the_whole_page() {
        let backend = SparseBatchKeyBackend {
            keys_seen: Cell::new(0),
            first_flush_at: Cell::new(None),
        };
        let parser = DocParser::new(&backend).with_region_batch_size(2);
        let layout = StaticLayout::new(
            (0..20)
                .map(|index| {
                    let width = 20.0 + index as f32;
                    let top = index as f32 * 3.0;
                    element("content", 0.0, top, width, top + 2.0)
                })
                .collect(),
        );

        let result = parser
            .parse(&layout, RgbImage::new(64, 64))
            .expect("parse succeeds");

        assert_eq!(result.layout_elements.len(), 20);
        assert_eq!(backend.first_flush_at.get(), Some(16));
    }

    #[test]
    fn propagates_one_batch_error_without_rewriting_it_per_item() {
        let parser = DocParser::new(&FailingBatchBackend).with_region_batch_size(2);
        let layout = StaticLayout::new(vec![
            element("text", 0.0, 0.0, 50.0, 20.0),
            element("text", 0.0, 30.0, 50.0, 50.0),
        ]);

        let error = parser
            .parse(&layout, RgbImage::new(64, 64))
            .expect_err("batch error must fail parsing");
        assert!(matches!(
            error,
            Error::Config { message } if message == "native batch failed"
        ));
    }

    /// An empty detection list falls back to whole-page recognition.
    #[test]
    fn falls_back_to_full_image_when_layout_is_empty() {
        let backend = RecordingBackend {
            crops: RefCell::new(Vec::new()),
        };
        let parser = DocParser::new(&backend);

        let result = parser
            .parse(&StaticLayout::new(Vec::new()), RgbImage::new(120, 80))
            .expect("parse succeeds");

        assert_eq!(result.layout_elements.len(), 1);
        assert_eq!(*backend.crops.borrow(), vec![(120, 80)]);
    }

    /// `parse_to_markdown` and `StructureResult::to_markdown` must drop the
    /// same labels; they used to disagree, so a page header reached the prose
    /// through one path but not the other.
    #[test]
    fn both_markdown_entry_points_skip_the_same_labels() {
        let backend = RecordingBackend {
            crops: RefCell::new(Vec::new()),
        };
        let config = DocParserConfig {
            // Keep the auxiliary regions so they reach the renderer at all.
            skip_auxiliary_regions: false,
            ..DocParserConfig::default()
        };
        let parser = DocParser::with_config(&backend, config);
        let layout = StaticLayout::new(vec![
            element("header", 10.0, 5.0, 190.0, 20.0),
            element("text", 10.0, 60.0, 190.0, 140.0),
        ]);

        let result = parser
            .parse(&layout, RgbImage::new(200, 200))
            .expect("parse succeeds");
        assert!(
            result
                .layout_elements
                .iter()
                .any(|e| e.element_type == LayoutElementType::Header),
            "the header must survive parsing for this test to mean anything"
        );

        let from_config = super::super::utils::to_markdown(
            &result.layout_elements,
            &parser.config().markdown_ignore_labels,
            parser.config().markdown_pretty,
        );
        assert_eq!(result.to_markdown(), from_config);
    }
}
