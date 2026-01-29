//! Template matching tool for locating images within screenshots.
//!
//! This module implements the `find_image` MCP tool which uses normalized
//! cross-correlation (NCC) to find template images within screenshots.

use crate::tools::image_cache::ImageCache;
use crate::tools::screenshot_cache::{ScreenshotCache, ScreenshotMetadata};
use base64::Engine;
use image::{GrayImage, ImageReader};
use rmcp::model::{CallToolResult, Content};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Parameters for the find_image tool.
#[derive(Debug, Deserialize)]
pub struct FindImageParams {
    /// Screenshot ID from a previous take_screenshot call.
    pub screenshot_id: Option<String>,

    /// Base64-encoded screenshot image (used if no screenshot_id).
    pub screenshot_image_base64: Option<String>,

    /// Image ID from a previous load_image call (preferred over template_image_base64).
    pub template_id: Option<String>,

    /// Base64-encoded template image to find (used if no template_id).
    pub template_image_base64: Option<String>,

    /// Image ID from a previous load_image call for the mask.
    pub mask_id: Option<String>,

    /// Base64-encoded mask image (optional; white=match, black=ignore).
    pub mask_image_base64: Option<String>,

    /// Mode: "fast" (default) or "accurate".
    #[serde(default = "default_mode")]
    pub mode: String,

    /// Minimum match score threshold (default depends on mode).
    pub threshold: Option<f64>,

    /// Maximum number of results to return (default depends on mode).
    pub max_results: Option<usize>,

    /// Scale search range: {min, max, step}.
    pub scales: Option<ScaleRange>,

    /// Rotations to try in degrees. Only 0, 90, 180, 270 are supported.
    pub rotations: Option<Vec<f64>>,

    /// Search region within the screenshot: {x, y, w, h}.
    pub search_region: Option<SearchRegion>,

    /// Stride for matching (default: 2 in fast mode, 1 in accurate mode).
    pub stride: Option<u32>,

    /// Return screen coordinates (requires screenshot metadata).
    #[serde(default = "default_return_screen_coords")]
    pub return_screen_coords: bool,
}

fn default_mode() -> String {
    "fast".to_string()
}

fn default_return_screen_coords() -> bool {
    true
}

/// Scale search range configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScaleRange {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl Default for ScaleRange {
    fn default() -> Self {
        Self {
            min: 0.8,
            max: 1.2,
            step: 0.1,
        }
    }
}

/// Search region within the screenshot.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchRegion {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A single match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    /// Match confidence score (0.0 to 1.0).
    pub score: f64,
    /// Bounding box in screenshot pixels.
    pub bbox: BoundingBox,
    /// Center point in screenshot pixels.
    pub center: Point,
    /// Scale at which the match was found.
    pub scale: f64,
    /// Rotation at which the match was found (degrees).
    pub rotation: f64,
    /// Screen X coordinate (if metadata available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_x: Option<f64>,
    /// Screen Y coordinate (if metadata available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_y: Option<f64>,
}

/// Bounding box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Point coordinate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Response from find_image tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct FindImageResponse {
    pub matches: Vec<MatchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Mode-specific defaults.
struct ModeDefaults {
    threshold: f64,
    max_results: usize,
    scales: ScaleRange,
    stride: u32,
}

impl ModeDefaults {
    fn fast() -> Self {
        Self {
            threshold: 0.88,
            max_results: 3,
            scales: ScaleRange {
                min: 0.8,
                max: 1.2,
                step: 0.1,
            },
            stride: 2,
        }
    }

    fn accurate() -> Self {
        Self {
            threshold: 0.85,
            max_results: 5,
            scales: ScaleRange {
                min: 0.5,
                max: 2.0,
                step: 0.05,
            },
            stride: 1,
        }
    }

    fn for_mode(mode: &str) -> Self {
        match mode {
            "accurate" => Self::accurate(),
            _ => Self::fast(),
        }
    }
}

/// Normalize a rotation angle to one of the supported values (0, 90, 180, 270).
/// Returns None if the angle is not within ±1° tolerance of a supported value.
fn normalize_rotation(r: f64) -> Option<f64> {
    let normalized = ((r % 360.0) + 360.0) % 360.0;
    // ±1° tolerance (inclusive)
    if normalized <= 1.0 || normalized >= 359.0 {
        Some(0.0)
    } else if (normalized - 90.0).abs() <= 1.0 {
        Some(90.0)
    } else if (normalized - 180.0).abs() <= 1.0 {
        Some(180.0)
    } else if (normalized - 270.0).abs() <= 1.0 {
        Some(270.0)
    } else {
        None
    }
}

/// Validate scale range parameters.
/// Returns Ok(()) if valid, Err(message) if invalid.
fn validate_scale_range(scales: &ScaleRange) -> Result<(), String> {
    if scales.step <= 0.0 {
        return Err("scales.step must be positive (got 0 or negative)".to_string());
    }
    if scales.min <= 0.0 {
        return Err(format!("scales.min must be positive (got {})", scales.min));
    }
    if scales.max <= 0.0 {
        return Err(format!("scales.max must be positive (got {})", scales.max));
    }
    if scales.min > scales.max {
        return Err(format!(
            "scales.min ({}) must not exceed scales.max ({})",
            scales.min, scales.max
        ));
    }
    Ok(())
}

/// Input data for the blocking matching operation.
struct MatchingInput {
    screenshot_png_data: Option<Vec<u8>>,
    screenshot_b64: Option<String>,
    template_png_data: Option<Vec<u8>>,
    template_b64: Option<String>,
    mask_png_data: Option<Vec<u8>>,
    mask_b64: Option<String>,
    search_region: Option<SearchRegion>,
    threshold: f64,
    max_results: usize,
    scales: ScaleRange,
    stride: u32,
    rotations: Vec<f64>,
    return_screen_coords: bool,
    screenshot_metadata: Option<ScreenshotMetadata>,
}

/// Result from the blocking matching operation.
enum MatchingResult {
    Success(Vec<MatchResult>),
    Error(String),
}

/// Execute the find_image tool.
pub async fn find_image(
    params: FindImageParams,
    screenshot_cache: Arc<RwLock<ScreenshotCache>>,
    image_cache: Arc<RwLock<ImageCache>>,
) -> CallToolResult {
    let defaults = ModeDefaults::for_mode(&params.mode);
    let mut warning: Option<String> = None;

    // Get parameters with mode defaults (cheap, keep on async thread)
    let threshold = params.threshold.unwrap_or(defaults.threshold);
    let max_results = params.max_results.unwrap_or(defaults.max_results);
    let scales = params.scales.clone().unwrap_or(defaults.scales);
    let stride = params.stride.unwrap_or(defaults.stride);
    let rotations = params.rotations.clone().unwrap_or_else(|| vec![0.0]);

    // Validate scale range to prevent infinite loops and degenerate cases
    if let Err(e) = validate_scale_range(&scales) {
        return CallToolResult::error(vec![Content::text(e)]);
    }

    // Validate, filter, and normalize rotations to exact {0, 90, 180, 270}
    let mut normalized_rotations = Vec::new();
    let mut invalid_rotations = Vec::new();

    for r in rotations {
        match normalize_rotation(r) {
            Some(exact) => {
                if !normalized_rotations.contains(&exact) {
                    normalized_rotations.push(exact);
                }
            }
            None => invalid_rotations.push(r),
        }
    }

    // Treat empty rotations as [0]
    let rotations = if normalized_rotations.is_empty() {
        vec![0.0]
    } else {
        normalized_rotations
    };

    // Warn about unsupported rotations
    if !invalid_rotations.is_empty() {
        let msg = format!(
            "Unsupported rotation angles ignored (only 0, 90, 180, 270 supported): {:?}",
            invalid_rotations
        );
        warning = Some(warning.map_or(msg.clone(), |w| format!("{}; {}", w, msg)));
    }

    // Validate search_region dimensions
    if let Some(region) = &params.search_region {
        if region.w == 0 || region.h == 0 {
            return CallToolResult::error(vec![Content::text(
                "search_region width and height must be positive",
            )]);
        }
    }

    // Resolve screenshot data from cache - clone bytes and release lock before decode
    let (screenshot_png_data, screenshot_metadata) = {
        if let Some(id) = &params.screenshot_id {
            let cache_guard = screenshot_cache.read().await;
            if let Some(cached) = cache_guard.peek(id) {
                // Clone the data so we can release the lock
                (Some(cached.png_data.clone()), Some(cached.metadata.clone()))
            } else {
                warning = Some(format!("Screenshot ID '{}' not found in cache", id));
                (None, None)
            }
        } else {
            (None, None)
        }
    };
    // Lock is now released

    // Resolve template data from image cache
    // Use write lock to update LRU access order via get()
    let template_png_data = {
        if let Some(id) = &params.template_id {
            let mut cache_guard = image_cache.write().await;
            if let Some(cached) = cache_guard.get(id) {
                Some(cached.png_data.clone())
            } else if params.template_image_base64.is_some() {
                // ID not found but base64 fallback available - warn and continue
                let msg = format!("Template ID '{}' not found in cache, using base64 fallback", id);
                warning = Some(warning.map_or(msg.clone(), |w| format!("{}; {}", w, msg)));
                None
            } else {
                return CallToolResult::error(vec![Content::text(format!(
                    "Template ID '{}' not found in image cache",
                    id
                ))]);
            }
        } else {
            None
        }
    };

    // Resolve mask data from image cache
    // Use write lock to update LRU access order via get()
    let mask_png_data = {
        if let Some(id) = &params.mask_id {
            let mut cache_guard = image_cache.write().await;
            if let Some(cached) = cache_guard.get(id) {
                Some(cached.png_data.clone())
            } else if params.mask_image_base64.is_some() {
                // ID not found but base64 fallback available - warn and continue
                let msg = format!("Mask ID '{}' not found in cache, using base64 fallback", id);
                warning = Some(warning.map_or(msg.clone(), |w| format!("{}; {}", w, msg)));
                None
            } else {
                return CallToolResult::error(vec![Content::text(format!(
                    "Mask ID '{}' not found in image cache",
                    id
                ))]);
            }
        } else {
            None
        }
    };

    // Validate that we have a template source
    if params.template_id.is_none() && params.template_image_base64.is_none() {
        return CallToolResult::error(vec![Content::text(
            "Either template_id or template_image_base64 must be provided",
        )]);
    }

    // Prepare input for blocking operation
    let input = MatchingInput {
        screenshot_png_data,
        screenshot_b64: params.screenshot_image_base64.clone(),
        template_png_data,
        template_b64: params.template_image_base64.clone(),
        mask_png_data,
        mask_b64: params.mask_image_base64.clone(),
        search_region: params.search_region.clone(),
        threshold,
        max_results,
        scales,
        stride,
        rotations,
        return_screen_coords: params.return_screen_coords,
        screenshot_metadata,
    };

    // Move heavy CPU work to a blocking thread
    let result = tokio::task::spawn_blocking(move || run_matching(input))
        .await
        .unwrap_or_else(|e| MatchingResult::Error(format!("Task panicked: {}", e)));

    match result {
        MatchingResult::Success(matches) => {
            let response = FindImageResponse { matches, warning };
            match serde_json::to_string_pretty(&response) {
                Ok(json) => CallToolResult::success(vec![Content::text(json)]),
                Err(e) => CallToolResult::error(vec![Content::text(format!(
                    "Failed to serialize response: {}",
                    e
                ))]),
            }
        }
        MatchingResult::Error(e) => CallToolResult::error(vec![Content::text(e)]),
    }
}

/// CPU-intensive matching logic, runs on a blocking thread.
fn run_matching(input: MatchingInput) -> MatchingResult {
    // Decode screenshot
    let (screenshot_gray, screenshot_metadata) = if let Some(png_data) = input.screenshot_png_data {
        match decode_png_to_gray(&png_data) {
            Ok(gray) => (gray, input.screenshot_metadata),
            Err(e) => {
                return MatchingResult::Error(format!("Failed to decode cached screenshot: {}", e))
            }
        }
    } else if let Some(b64) = &input.screenshot_b64 {
        match decode_base64_to_gray(b64) {
            Ok(gray) => (gray, None),
            Err(e) => return MatchingResult::Error(format!("Failed to decode screenshot: {}", e)),
        }
    } else {
        return MatchingResult::Error(
            "Either screenshot_id or screenshot_image_base64 must be provided".to_string(),
        );
    };

    // Decode template image (prefer cached PNG data over base64)
    let template_gray = if let Some(png_data) = input.template_png_data {
        match decode_png_to_gray(&png_data) {
            Ok(img) => img,
            Err(e) => {
                return MatchingResult::Error(format!("Failed to decode cached template: {}", e))
            }
        }
    } else if let Some(b64) = &input.template_b64 {
        match decode_base64_to_gray(b64) {
            Ok(img) => img,
            Err(e) => {
                return MatchingResult::Error(format!("Failed to decode template image: {}", e))
            }
        }
    } else {
        return MatchingResult::Error(
            "Either template_id or template_image_base64 must be provided".to_string(),
        );
    };

    // Decode mask if provided (prefer cached PNG data over base64)
    let mask = if let Some(png_data) = input.mask_png_data {
        match decode_png_to_gray(&png_data) {
            Ok(img) => Some(img),
            Err(e) => return MatchingResult::Error(format!("Failed to decode cached mask: {}", e)),
        }
    } else if let Some(mask_b64) = &input.mask_b64 {
        match decode_base64_to_gray(mask_b64) {
            Ok(img) => Some(img),
            Err(e) => return MatchingResult::Error(format!("Failed to decode mask image: {}", e)),
        }
    } else {
        None
    };

    // Validate mask dimensions match template
    if let Some(mask_img) = &mask {
        if mask_img.width() != template_gray.width() || mask_img.height() != template_gray.height()
        {
            return MatchingResult::Error(format!(
                "Mask dimensions ({}x{}) must match template dimensions ({}x{})",
                mask_img.width(),
                mask_img.height(),
                template_gray.width(),
                template_gray.height()
            ));
        }
    }

    // Extract search region if specified
    let search_img = if let Some(region) = &input.search_region {
        extract_region(&screenshot_gray, region)
    } else {
        screenshot_gray.clone()
    };
    let region_offset = input
        .search_region
        .as_ref()
        .map(|r| (r.x, r.y))
        .unwrap_or((0, 0));

    // Run multi-scale template matching
    let mut all_matches = Vec::new();

    for rotation in &input.rotations {
        let rotated_template = if *rotation != 0.0 {
            rotate_image(&template_gray, *rotation)
        } else {
            template_gray.clone()
        };

        let rotated_mask = if *rotation != 0.0 {
            mask.as_ref().map(|m| rotate_image(m, *rotation))
        } else {
            mask.clone()
        };

        let mut scale = input.scales.min;
        while scale <= input.scales.max + f64::EPSILON {
            let scaled_template = resize_image(&rotated_template, scale);
            let scaled_mask = rotated_mask.as_ref().map(|m| resize_image(m, scale));

            // Skip if template is larger than search image
            if scaled_template.width() > search_img.width()
                || scaled_template.height() > search_img.height()
            {
                scale += input.scales.step;
                continue;
            }

            // Run NCC matching
            let matches = match_template_ncc(
                &search_img,
                &scaled_template,
                scaled_mask.as_ref(),
                input.threshold,
                input.stride,
            );

            // Convert to MatchResult with adjusted coordinates
            for (x, y, score) in matches {
                let adjusted_x = x + region_offset.0;
                let adjusted_y = y + region_offset.1;
                let tw = scaled_template.width();
                let th = scaled_template.height();

                let center_x = adjusted_x as f64 + tw as f64 / 2.0;
                let center_y = adjusted_y as f64 + th as f64 / 2.0;

                // Convert to screen coordinates if metadata available
                let (screen_x, screen_y) = if input.return_screen_coords {
                    if let Some(meta) = &screenshot_metadata {
                        let sx = meta.origin_x + center_x / meta.scale;
                        let sy = meta.origin_y + center_y / meta.scale;
                        (Some(sx), Some(sy))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                all_matches.push(MatchResult {
                    score,
                    bbox: BoundingBox {
                        x: adjusted_x,
                        y: adjusted_y,
                        w: tw,
                        h: th,
                    },
                    center: Point {
                        x: center_x,
                        y: center_y,
                    },
                    scale,
                    rotation: *rotation,
                    screen_x,
                    screen_y,
                });
            }

            scale += input.scales.step;
        }
    }

    // Apply Non-Maximum Suppression
    let final_matches = non_maximum_suppression(all_matches, 0.3, input.max_results);

    MatchingResult::Success(final_matches)
}

/// Decode base64 image data to grayscale.
fn decode_base64_to_gray(b64: &str) -> Result<GrayImage, String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("Invalid base64: {}", e))?;
    decode_png_to_gray(&data)
}

/// Decode PNG/JPEG bytes to grayscale.
fn decode_png_to_gray(data: &[u8]) -> Result<GrayImage, String> {
    let img = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("Failed to read image format: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    Ok(img.to_luma8())
}

/// Extract a region from an image.
fn extract_region(img: &GrayImage, region: &SearchRegion) -> GrayImage {
    let x = region.x.min(img.width().saturating_sub(1));
    let y = region.y.min(img.height().saturating_sub(1));
    let w = region.w.min(img.width() - x);
    let h = region.h.min(img.height() - y);

    let sub = image::imageops::crop_imm(img, x, y, w, h);
    sub.to_image()
}

/// Resize image by scale factor.
fn resize_image(img: &GrayImage, scale: f64) -> GrayImage {
    if (scale - 1.0).abs() < f64::EPSILON {
        return img.clone();
    }

    let new_width = ((img.width() as f64) * scale).round() as u32;
    let new_height = ((img.height() as f64) * scale).round() as u32;

    if new_width == 0 || new_height == 0 {
        return GrayImage::new(1, 1);
    }

    image::imageops::resize(
        img,
        new_width,
        new_height,
        image::imageops::FilterType::Triangle,
    )
}

/// Simple rotation (only supports 0, 90, 180, 270).
/// Expects normalized input from validation (exact 0.0, 90.0, 180.0, or 270.0).
fn rotate_image(img: &GrayImage, degrees: f64) -> GrayImage {
    // Use rounding to handle any floating point imprecision
    let rounded = degrees.round() as i32;
    match rounded {
        90 => image::imageops::rotate90(img),
        180 => image::imageops::rotate180(img),
        270 => image::imageops::rotate270(img),
        _ => img.clone(), // 0 or fallback
    }
}

/// Normalized Cross-Correlation template matching.
///
/// Returns a list of (x, y, score) for matches above threshold.
fn match_template_ncc(
    image: &GrayImage,
    template: &GrayImage,
    mask: Option<&GrayImage>,
    threshold: f64,
    stride: u32,
) -> Vec<(u32, u32, f64)> {
    let img_w = image.width();
    let img_h = image.height();
    let tpl_w = template.width();
    let tpl_h = template.height();

    if tpl_w > img_w || tpl_h > img_h {
        return Vec::new();
    }

    let stride = stride.max(1);

    // Precompute template statistics
    let tpl_stats = compute_template_stats(template, mask);

    if tpl_stats.std < f64::EPSILON || tpl_stats.pixel_count == 0 {
        return Vec::new();
    }

    let mut matches = Vec::new();

    let search_w = img_w - tpl_w + 1;
    let search_h = img_h - tpl_h + 1;

    // Iterate over search positions with stride
    let mut y = 0u32;
    while y < search_h {
        let mut x = 0u32;
        while x < search_w {
            let score = compute_ncc_at(image, template, mask, x, y, &tpl_stats);

            if score >= threshold {
                matches.push((x, y, score));
            }

            x += stride;
        }
        y += stride;
    }

    matches
}

/// Precomputed template statistics for NCC matching.
struct TemplateStats {
    mean: f64,
    std: f64,
    pixel_count: usize,
}

/// Compute template mean, std deviation, and pixel count.
fn compute_template_stats(template: &GrayImage, mask: Option<&GrayImage>) -> TemplateStats {
    let mut sum = 0.0;
    let mut sum_sq = 0.0;
    let mut count = 0usize;

    for (x, y, pixel) in template.enumerate_pixels() {
        let use_pixel = mask
            .map(|m| m.get_pixel(x.min(m.width() - 1), y.min(m.height() - 1)).0[0] > 128)
            .unwrap_or(true);

        if use_pixel {
            let val = pixel.0[0] as f64;
            sum += val;
            sum_sq += val * val;
            count += 1;
        }
    }

    if count == 0 {
        return TemplateStats {
            mean: 0.0,
            std: 0.0,
            pixel_count: 0,
        };
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    let std = variance.max(0.0).sqrt();

    TemplateStats {
        mean,
        std,
        pixel_count: count,
    }
}

/// Compute NCC score at a specific position.
#[allow(clippy::too_many_arguments)]
fn compute_ncc_at(
    image: &GrayImage,
    template: &GrayImage,
    mask: Option<&GrayImage>,
    offset_x: u32,
    offset_y: u32,
    tpl_stats: &TemplateStats,
) -> f64 {
    let tpl_w = template.width();
    let tpl_h = template.height();

    // Compute image region statistics
    let mut img_sum = 0.0;
    let mut img_sum_sq = 0.0;
    let mut cross_sum = 0.0;

    for ty in 0..tpl_h {
        for tx in 0..tpl_w {
            let use_pixel = mask
                .map(|m| m.get_pixel(tx.min(m.width() - 1), ty.min(m.height() - 1)).0[0] > 128)
                .unwrap_or(true);

            if use_pixel {
                let img_val = image.get_pixel(offset_x + tx, offset_y + ty).0[0] as f64;
                let tpl_val = template.get_pixel(tx, ty).0[0] as f64;

                img_sum += img_val;
                img_sum_sq += img_val * img_val;
                cross_sum += img_val * tpl_val;
            }
        }
    }

    let count = tpl_stats.pixel_count as f64;
    let img_mean = img_sum / count;
    let img_variance = (img_sum_sq / count) - (img_mean * img_mean);
    let img_std = img_variance.max(0.0).sqrt();

    if img_std < f64::EPSILON {
        return 0.0;
    }

    // NCC = sum((I - mean_I) * (T - mean_T)) / (n * std_I * std_T)
    // Expanded: (sum(I*T) - n*mean_I*mean_T) / (n * std_I * std_T)
    let numerator = cross_sum - count * img_mean * tpl_stats.mean;
    let denominator = count * img_std * tpl_stats.std;

    if denominator < f64::EPSILON {
        return 0.0;
    }

    (numerator / denominator).clamp(-1.0, 1.0)
}

/// Non-Maximum Suppression to remove overlapping detections.
fn non_maximum_suppression(
    mut matches: Vec<MatchResult>,
    iou_threshold: f64,
    max_results: usize,
) -> Vec<MatchResult> {
    // Sort by score descending
    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep = Vec::new();

    while !matches.is_empty() && keep.len() < max_results {
        let best = matches.remove(0);

        // Remove all matches that overlap too much with the best
        matches.retain(|m| compute_iou(&best.bbox, &m.bbox) < iou_threshold);

        keep.push(best);
    }

    keep
}

/// Compute Intersection over Union of two bounding boxes.
fn compute_iou(a: &BoundingBox, b: &BoundingBox) -> f64 {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.w).min(b.x + b.w);
    let y2 = (a.y + a.h).min(b.y + b.h);

    if x2 <= x1 || y2 <= y1 {
        return 0.0;
    }

    let intersection = (x2 - x1) as f64 * (y2 - y1) as f64;
    let area_a = a.w as f64 * a.h as f64;
    let area_b = b.w as f64 * b.h as f64;
    let union = area_a + area_b - intersection;

    if union < f64::EPSILON {
        return 0.0;
    }

    intersection / union
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn test_compute_iou() {
        // Identical boxes
        let a = BoundingBox {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let b = BoundingBox {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        assert!((compute_iou(&a, &b) - 1.0).abs() < f64::EPSILON);

        // No overlap
        let c = BoundingBox {
            x: 20,
            y: 20,
            w: 10,
            h: 10,
        };
        assert!((compute_iou(&a, &c)).abs() < f64::EPSILON);

        // Partial overlap (50%)
        let d = BoundingBox {
            x: 5,
            y: 0,
            w: 10,
            h: 10,
        };
        // Intersection: 5x10 = 50, Union: 100 + 100 - 50 = 150
        let iou = compute_iou(&a, &d);
        assert!((iou - 50.0 / 150.0).abs() < 0.01);
    }

    #[test]
    fn test_resize_image() {
        let img = GrayImage::from_fn(10, 10, |_, _| Luma([128]));

        let scaled = resize_image(&img, 2.0);
        assert_eq!(scaled.width(), 20);
        assert_eq!(scaled.height(), 20);

        let scaled_down = resize_image(&img, 0.5);
        assert_eq!(scaled_down.width(), 5);
        assert_eq!(scaled_down.height(), 5);
    }

    #[test]
    fn test_template_stats() {
        // Uniform image
        let img = GrayImage::from_fn(5, 5, |_, _| Luma([100]));
        let stats = compute_template_stats(&img, None);
        assert!((stats.mean - 100.0).abs() < f64::EPSILON);
        assert!(stats.std < f64::EPSILON); // Uniform -> std = 0
        assert_eq!(stats.pixel_count, 25);
    }

    #[test]
    fn test_nms() {
        let matches = vec![
            MatchResult {
                score: 0.9,
                bbox: BoundingBox {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 10,
                },
                center: Point { x: 5.0, y: 5.0 },
                scale: 1.0,
                rotation: 0.0,
                screen_x: None,
                screen_y: None,
            },
            MatchResult {
                score: 0.85,
                bbox: BoundingBox {
                    x: 2,
                    y: 2,
                    w: 10,
                    h: 10,
                },
                center: Point { x: 7.0, y: 7.0 },
                scale: 1.0,
                rotation: 0.0,
                screen_x: None,
                screen_y: None,
            },
            MatchResult {
                score: 0.8,
                bbox: BoundingBox {
                    x: 50,
                    y: 50,
                    w: 10,
                    h: 10,
                },
                center: Point { x: 55.0, y: 55.0 },
                scale: 1.0,
                rotation: 0.0,
                screen_x: None,
                screen_y: None,
            },
        ];

        let result = non_maximum_suppression(matches, 0.3, 5);
        // First two overlap significantly, third doesn't
        assert_eq!(result.len(), 2);
        assert!((result[0].score - 0.9).abs() < f64::EPSILON);
        assert!((result[1].score - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ncc_matching_finds_exact_template() {
        // Create a 100x100 image with a distinct gradient pattern at (30, 40)
        let mut image = GrayImage::from_fn(100, 100, |_, _| Luma([128]));

        // Draw a gradient pattern at position (30, 40) - needs variation for NCC
        for y in 0..10u32 {
            for x in 0..10u32 {
                // Create a diagonal gradient pattern
                let val = ((x + y) * 12 + 50) as u8;
                image.put_pixel(30 + x, 40 + y, Luma([val]));
            }
        }

        // Create template that matches the pattern exactly
        let template = GrayImage::from_fn(10, 10, |x, y| {
            let val = ((x + y) * 12 + 50) as u8;
            Luma([val])
        });

        // Run NCC matching
        let matches = match_template_ncc(&image, &template, None, 0.9, 1);

        // Should find the match
        assert!(!matches.is_empty(), "Should find at least one match");

        // Best match should be near (30, 40)
        let (best_x, best_y, best_score) = matches
            .iter()
            .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
            .cloned()
            .unwrap();

        assert!(
            best_score > 0.95,
            "Score should be very high for exact match, got {}",
            best_score
        );
        assert_eq!(best_x, 30, "X should be 30, got {}", best_x);
        assert_eq!(best_y, 40, "Y should be 40, got {}", best_y);
    }

    #[test]
    fn test_ncc_matching_with_stride() {
        // Create a 50x50 image with a patterned 6x6 template
        let mut image = GrayImage::from_fn(50, 50, |_, _| Luma([100]));

        // Draw a checkerboard-like pattern at (20, 20)
        for y in 0..6u32 {
            for x in 0..6u32 {
                let val = if (x + y) % 2 == 0 { 200u8 } else { 50u8 };
                image.put_pixel(20 + x, 20 + y, Luma([val]));
            }
        }

        // Create matching template
        let template = GrayImage::from_fn(6, 6, |x, y| {
            let val = if (x + y) % 2 == 0 { 200u8 } else { 50u8 };
            Luma([val])
        });

        // With stride=2, we should still find the match (20 is divisible by 2)
        let matches = match_template_ncc(&image, &template, None, 0.8, 2);

        assert!(!matches.is_empty(), "Should find match with stride=2");

        // Find best match
        let (best_x, best_y, _) = matches
            .iter()
            .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
            .cloned()
            .unwrap();

        // Should be within stride distance of actual position
        assert!(
            (best_x as i32 - 20).abs() <= 2,
            "X should be near 20, got {}",
            best_x
        );
        assert!(
            (best_y as i32 - 20).abs() <= 2,
            "Y should be near 20, got {}",
            best_y
        );
    }

    #[test]
    fn test_ncc_no_match_for_different_pattern() {
        // Create image with a horizontal gradient
        let image = GrayImage::from_fn(50, 50, |x, _| Luma([(x * 5) as u8]));

        // Template with vertical gradient (orthogonal pattern)
        let template = GrayImage::from_fn(10, 10, |_, y| Luma([(y * 25) as u8]));

        // High threshold, should find no good matches for orthogonal patterns
        let matches = match_template_ncc(&image, &template, None, 0.9, 1);

        // NCC can produce negative correlation for orthogonal patterns
        // so we check that high-threshold matches are not found
        assert!(
            matches.is_empty(),
            "Should not find match for orthogonal pattern at high threshold, got {} matches",
            matches.len()
        );
    }

    #[test]
    fn test_scale_range_default() {
        let range = ScaleRange::default();
        assert!((range.min - 0.8).abs() < f64::EPSILON);
        assert!((range.max - 1.2).abs() < f64::EPSILON);
        assert!((range.step - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_region_clamps_to_bounds() {
        let img = GrayImage::from_fn(100, 100, |x, y| Luma([(x + y) as u8]));

        // Region exceeds image bounds
        let region = SearchRegion {
            x: 80,
            y: 90,
            w: 50,
            h: 50,
        };
        let extracted = extract_region(&img, &region);

        // Should be clamped to available space
        assert_eq!(extracted.width(), 20); // 100 - 80
        assert_eq!(extracted.height(), 10); // 100 - 90
    }

    #[test]
    fn test_extract_region_at_edge() {
        let img = GrayImage::from_fn(50, 50, |_, _| Luma([128]));

        // Region starting at max valid position
        let region = SearchRegion {
            x: 49,
            y: 49,
            w: 10,
            h: 10,
        };
        let extracted = extract_region(&img, &region);

        // Should extract 1x1 (the only pixel available)
        assert_eq!(extracted.width(), 1);
        assert_eq!(extracted.height(), 1);
    }

    #[test]
    fn test_rotate_image_90_degrees() {
        // Create a simple 2x3 image
        let img = GrayImage::from_vec(2, 3, vec![1, 2, 3, 4, 5, 6]).unwrap();
        let rotated = rotate_image(&img, 90.0);

        // After 90 degree rotation, 2x3 becomes 3x2
        assert_eq!(rotated.width(), 3);
        assert_eq!(rotated.height(), 2);
    }

    #[test]
    fn test_rotate_image_unsupported_angle() {
        let img = GrayImage::from_fn(10, 10, |_, _| Luma([128]));

        // 45 degrees is not supported, should return unchanged
        let rotated = rotate_image(&img, 45.0);
        assert_eq!(rotated.dimensions(), img.dimensions());
    }

    #[test]
    fn test_resize_image_zero_scale() {
        let img = GrayImage::from_fn(10, 10, |_, _| Luma([128]));

        // Very small scale should produce 1x1 minimum
        let tiny = resize_image(&img, 0.01);
        assert!(tiny.width() >= 1);
        assert!(tiny.height() >= 1);
    }

    #[test]
    fn test_rotate_image_with_normalized_values() {
        let img = GrayImage::from_vec(2, 3, vec![1, 2, 3, 4, 5, 6]).unwrap();

        // Test all supported normalized values
        let rot0 = rotate_image(&img, 0.0);
        assert_eq!(rot0.dimensions(), (2, 3));

        let rot90 = rotate_image(&img, 90.0);
        assert_eq!(rot90.dimensions(), (3, 2));

        let rot180 = rotate_image(&img, 180.0);
        assert_eq!(rot180.dimensions(), (2, 3));

        let rot270 = rotate_image(&img, 270.0);
        assert_eq!(rot270.dimensions(), (3, 2));
    }

    // Tests now use the production normalize_rotation function from super::*

    #[test]
    fn test_rotation_normalization_exact_values() {
        assert_eq!(normalize_rotation(0.0), Some(0.0));
        assert_eq!(normalize_rotation(90.0), Some(90.0));
        assert_eq!(normalize_rotation(180.0), Some(180.0));
        assert_eq!(normalize_rotation(270.0), Some(270.0));
    }

    #[test]
    fn test_rotation_normalization_wrapping() {
        // Values that wrap around 360
        assert_eq!(normalize_rotation(360.0), Some(0.0));
        assert_eq!(normalize_rotation(450.0), Some(90.0));
        assert_eq!(normalize_rotation(540.0), Some(180.0));
        assert_eq!(normalize_rotation(-90.0), Some(270.0));
        assert_eq!(normalize_rotation(-270.0), Some(90.0));
    }

    #[test]
    fn test_rotation_normalization_tolerance() {
        // Values within ±1° tolerance should normalize
        assert_eq!(normalize_rotation(89.5), Some(90.0));
        assert_eq!(normalize_rotation(90.5), Some(90.0));
        assert_eq!(normalize_rotation(0.5), Some(0.0));
        assert_eq!(normalize_rotation(359.5), Some(0.0));
        assert_eq!(normalize_rotation(179.9), Some(180.0));
        assert_eq!(normalize_rotation(270.9), Some(270.0));

        // Boundary: exactly 1° away should be accepted (inclusive)
        assert_eq!(normalize_rotation(89.0), Some(90.0));
        assert_eq!(normalize_rotation(91.0), Some(90.0));
        assert_eq!(normalize_rotation(1.0), Some(0.0));
        assert_eq!(normalize_rotation(359.0), Some(0.0));
        assert_eq!(normalize_rotation(179.0), Some(180.0));
        assert_eq!(normalize_rotation(181.0), Some(180.0));
        assert_eq!(normalize_rotation(269.0), Some(270.0));
        assert_eq!(normalize_rotation(271.0), Some(270.0));
    }

    #[test]
    fn test_rotation_normalization_invalid() {
        // Values outside tolerance should be rejected
        assert_eq!(normalize_rotation(45.0), None);
        assert_eq!(normalize_rotation(135.0), None);
        assert_eq!(normalize_rotation(87.9), None); // > 1° from 90
        assert_eq!(normalize_rotation(92.1), None); // > 1° from 90
    }

    #[test]
    fn test_scale_validation_valid_ranges() {
        // Normal valid range
        assert!(validate_scale_range(&ScaleRange {
            min: 0.5,
            max: 2.0,
            step: 0.1,
        })
        .is_ok());

        // Single-scale search (min == max) is valid
        assert!(validate_scale_range(&ScaleRange {
            min: 1.0,
            max: 1.0,
            step: 0.1,
        })
        .is_ok());

        // Defaults should be valid
        assert!(validate_scale_range(&ScaleRange::default()).is_ok());
    }

    #[test]
    fn test_scale_validation_step_must_be_positive() {
        // Zero step
        let result = validate_scale_range(&ScaleRange {
            min: 0.8,
            max: 1.2,
            step: 0.0,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("step"));

        // Negative step
        let result = validate_scale_range(&ScaleRange {
            min: 0.8,
            max: 1.2,
            step: -0.1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("step"));
    }

    #[test]
    fn test_scale_validation_min_must_be_positive() {
        // Zero min
        let result = validate_scale_range(&ScaleRange {
            min: 0.0,
            max: 1.2,
            step: 0.1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("min"));

        // Negative min
        let result = validate_scale_range(&ScaleRange {
            min: -0.5,
            max: 1.2,
            step: 0.1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("min"));
    }

    #[test]
    fn test_scale_validation_max_must_be_positive() {
        // Zero max
        let result = validate_scale_range(&ScaleRange {
            min: 0.5,
            max: 0.0,
            step: 0.1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max"));

        // Negative max
        let result = validate_scale_range(&ScaleRange {
            min: 0.5,
            max: -1.0,
            step: 0.1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max"));
    }

    #[test]
    fn test_scale_validation_min_not_exceed_max() {
        let result = validate_scale_range(&ScaleRange {
            min: 2.0,
            max: 0.5,
            step: 0.1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not exceed"));
    }

    // ============ Integration tests for template_id / mask_id ============

    use crate::tools::image_cache::{ImageCache, ImageMetadata};
    use image::GenericImage;
    use std::io::Cursor;

    /// Create a test PNG in memory and return bytes
    fn create_test_png_bytes(width: u32, height: u32) -> Vec<u8> {
        let img = image::DynamicImage::new_rgb8(width, height);
        let mut cursor = Cursor::new(Vec::new());
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        cursor.into_inner()
    }

    fn make_image_metadata(width: u32, height: u32) -> ImageMetadata {
        ImageMetadata {
            source_path: None,
            width,
            height,
            channels: 3,
            mime: "image/png".to_string(),
            sha256: None,
            is_mask: false,
        }
    }

    #[tokio::test]
    async fn test_find_image_missing_template_id_errors() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(100, 100)),
            ),
            template_id: Some("nonexistent-id".to_string()),
            template_image_base64: None,
            mask_id: None,
            mask_image_base64: None,
            mode: "fast".to_string(),
            threshold: None,
            max_results: None,
            scales: None,
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(result.is_error.unwrap_or(false));
        // Check error message mentions template ID
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            assert!(text.contains("Template ID") && text.contains("not found"));
        }
    }

    #[tokio::test]
    async fn test_find_image_missing_mask_id_errors() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        // Add a template to the cache
        {
            let mut cache = image_cache.write().await;
            cache.store(
                create_test_png_bytes(32, 32),
                make_image_metadata(32, 32),
                Some("template"),
            );
        }

        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(100, 100)),
            ),
            template_id: None,
            template_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(16, 16)),
            ),
            mask_id: Some("nonexistent-mask".to_string()),
            mask_image_base64: None,
            mode: "fast".to_string(),
            threshold: None,
            max_results: None,
            scales: None,
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(result.is_error.unwrap_or(false));
        // Check error message mentions mask ID
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            assert!(text.contains("Mask ID") && text.contains("not found"));
        }
    }

    #[tokio::test]
    async fn test_find_image_requires_template_source() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(100, 100)),
            ),
            template_id: None,
            template_image_base64: None, // Neither template_id nor template_image_base64
            mask_id: None,
            mask_image_base64: None,
            mode: "fast".to_string(),
            threshold: None,
            max_results: None,
            scales: None,
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(result.is_error.unwrap_or(false));
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            assert!(text.contains("template_id") || text.contains("template_image_base64"));
        }
    }

    #[tokio::test]
    async fn test_find_image_with_template_id_from_cache() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        // Create a pattern with variation (gradient) that NCC can match
        let mut screenshot_img = image::DynamicImage::new_rgb8(100, 100);
        // Fill with gray background
        for y in 0..100u32 {
            for x in 0..100u32 {
                screenshot_img.put_pixel(x, y, image::Rgba([128, 128, 128, 255]));
            }
        }
        // Draw a diagonal gradient pattern at (30, 40)
        for y in 0..10u32 {
            for x in 0..10u32 {
                let val = ((x + y) * 12 + 50) as u8;
                screenshot_img.put_pixel(30 + x, 40 + y, image::Rgba([val, val, val, 255]));
            }
        }
        let mut screenshot_bytes = Cursor::new(Vec::new());
        screenshot_img
            .write_to(&mut screenshot_bytes, image::ImageFormat::Png)
            .unwrap();

        // Create template matching the gradient pattern
        let mut template_img = image::DynamicImage::new_rgb8(10, 10);
        for y in 0..10u32 {
            for x in 0..10u32 {
                let val = ((x + y) * 12 + 50) as u8;
                template_img.put_pixel(x, y, image::Rgba([val, val, val, 255]));
            }
        }
        let mut template_bytes = Cursor::new(Vec::new());
        template_img
            .write_to(&mut template_bytes, image::ImageFormat::Png)
            .unwrap();

        // Add template to cache
        let template_id = {
            let mut cache = image_cache.write().await;
            cache.store(
                template_bytes.into_inner(),
                make_image_metadata(10, 10),
                Some("template"),
            )
        };

        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(screenshot_bytes.into_inner()),
            ),
            template_id: Some(template_id),
            template_image_base64: None,
            mask_id: None,
            mask_image_base64: None,
            mode: "fast".to_string(),
            threshold: Some(0.9),
            max_results: None,
            scales: Some(ScaleRange {
                min: 1.0,
                max: 1.0,
                step: 0.1,
            }), // Exact scale only
            rotations: None,
            search_region: None,
            stride: Some(1),
            return_screen_coords: false,
        };

        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(
            !result.is_error.unwrap_or(true),
            "find_image should succeed"
        );

        // Parse response and verify we got a match
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            let response: FindImageResponse = serde_json::from_str(text).unwrap();
            assert!(
                !response.matches.is_empty(),
                "Should find at least one match"
            );
            // The match should be near (30, 40)
            let best = &response.matches[0];
            assert!(
                best.bbox.x >= 28 && best.bbox.x <= 32,
                "x should be near 30, got {}",
                best.bbox.x
            );
            assert!(
                best.bbox.y >= 38 && best.bbox.y <= 42,
                "y should be near 40, got {}",
                best.bbox.y
            );
        }
    }

    #[tokio::test]
    async fn test_find_image_template_id_preferred_over_base64() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        // Add a small template to the cache
        let template_id = {
            let mut cache = image_cache.write().await;
            cache.store(
                create_test_png_bytes(8, 8),
                make_image_metadata(8, 8),
                Some("template"),
            )
        };

        // Provide both template_id and template_image_base64 - template_id should be used
        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(50, 50)),
            ),
            template_id: Some(template_id.clone()),
            template_image_base64: Some("invalid_base64_that_would_fail".to_string()), // Would error if used
            mask_id: None,
            mask_image_base64: None,
            mode: "fast".to_string(),
            threshold: Some(0.5),
            max_results: None,
            scales: Some(ScaleRange {
                min: 1.0,
                max: 1.0,
                step: 0.1,
            }),
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        // Should succeed because template_id is used, not the invalid base64
        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(
            !result.is_error.unwrap_or(true),
            "Should succeed using template_id"
        );
    }

    #[tokio::test]
    async fn test_find_image_stale_template_id_falls_back_to_base64() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        // Provide a stale template_id but valid base64 fallback
        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(50, 50)),
            ),
            template_id: Some("stale-id-not-in-cache".to_string()),
            template_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(8, 8)),
            ),
            mask_id: None,
            mask_image_base64: None,
            mode: "fast".to_string(),
            threshold: Some(0.5),
            max_results: None,
            scales: Some(ScaleRange {
                min: 1.0,
                max: 1.0,
                step: 0.1,
            }),
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        // Should succeed using base64 fallback with a warning
        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(
            !result.is_error.unwrap_or(true),
            "Should succeed using base64 fallback"
        );

        // Verify warning is present
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            let response: FindImageResponse = serde_json::from_str(text).unwrap();
            assert!(response.warning.is_some(), "Should have warning about stale ID");
            assert!(
                response.warning.as_ref().unwrap().contains("not found in cache"),
                "Warning should mention cache miss"
            );
        }
    }

    #[tokio::test]
    async fn test_find_image_stale_mask_id_falls_back_to_base64() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        // Provide valid template but stale mask_id with base64 fallback
        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(50, 50)),
            ),
            template_id: None,
            template_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(8, 8)),
            ),
            mask_id: Some("stale-mask-id".to_string()),
            mask_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(8, 8)),
            ),
            mode: "fast".to_string(),
            threshold: Some(0.5),
            max_results: None,
            scales: Some(ScaleRange {
                min: 1.0,
                max: 1.0,
                step: 0.1,
            }),
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        // Should succeed using base64 fallback
        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(
            !result.is_error.unwrap_or(true),
            "Should succeed using mask base64 fallback"
        );

        // Verify warning is present
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            let response: FindImageResponse = serde_json::from_str(text).unwrap();
            assert!(response.warning.is_some(), "Should have warning about stale mask ID");
            assert!(
                response.warning.as_ref().unwrap().contains("Mask ID"),
                "Warning should mention mask"
            );
        }
    }

    #[tokio::test]
    async fn test_find_image_mask_dimension_mismatch_errors() {
        let screenshot_cache = Arc::new(RwLock::new(ScreenshotCache::default()));
        let image_cache = Arc::new(RwLock::new(ImageCache::default()));

        // Template is 10x10, mask is 8x8 - dimension mismatch
        let params = FindImageParams {
            screenshot_id: None,
            screenshot_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(50, 50)),
            ),
            template_id: None,
            template_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(10, 10)),
            ),
            mask_id: None,
            mask_image_base64: Some(
                base64::engine::general_purpose::STANDARD.encode(create_test_png_bytes(8, 8)),
            ),
            mode: "fast".to_string(),
            threshold: None,
            max_results: None,
            scales: Some(ScaleRange {
                min: 1.0,
                max: 1.0,
                step: 0.1,
            }),
            rotations: None,
            search_region: None,
            stride: None,
            return_screen_coords: false,
        };

        let result = find_image(params, screenshot_cache, image_cache).await;
        assert!(result.is_error.unwrap_or(false), "Should error on mask dimension mismatch");

        // Verify error message mentions dimension mismatch
        if let rmcp::model::RawContent::Text(rmcp::model::RawTextContent { text }) =
            &result.content[0].raw
        {
            assert!(
                text.contains("Mask dimensions") && text.contains("must match"),
                "Error should mention mask dimension mismatch, got: {}",
                text
            );
        }
    }
}
