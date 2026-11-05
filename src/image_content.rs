use base64::Engine;
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use std::io::Cursor;

pub const MAX_IMAGES_PER_REQUEST: usize = 4;
pub const MAX_COMPRESSED_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;
pub const MAX_DECODED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_NORMALIZED_IMAGE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct RawClipboardImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl std::fmt::Debug for RawClipboardImage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RawClipboardImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("payload", &"[OMITTED]")
            .finish()
    }
}

impl RawClipboardImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        validate_dimensions(width, height)?;
        let expected = u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4);
        if expected > MAX_DECODED_IMAGE_BYTES || rgba.len() as u64 != expected {
            return Err("clipboard image has invalid or oversized pixel data".into());
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn normalize(self) -> Result<ModelImage, String> {
        normalize_rgba(self.width, self.height, self.rgba)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ModelImage {
    png: std::sync::Arc<[u8]>,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for ModelImage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelImage")
            .field("media_type", &"image/png")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("payload", &"[OMITTED]")
            .finish()
    }
}

impl ModelImage {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn media_type(&self) -> &'static str {
        "image/png"
    }

    pub fn safe_marker(&self) -> String {
        format!(
            "[image/png {}×{}; image available only in this process]",
            self.width, self.height
        )
    }

    pub fn unavailable_marker(&self) -> String {
        format!(
            "[Attached image: image/png {}×{}; unavailable after this process]",
            self.width, self.height
        )
    }

    pub(crate) fn encoded_len(&self) -> usize {
        self.png.len()
    }

    pub(crate) fn png_bytes(&self) -> &[u8] {
        &self.png
    }

    pub(crate) fn base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.png)
    }

    pub(crate) fn data_url(&self) -> String {
        format!("data:image/png;base64,{}", self.base64())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelContentBlock {
    Text(String),
    Image(ModelImage),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelContent {
    blocks: Vec<ModelContentBlock>,
    prunable_tool_result: bool,
    tool_success: Option<bool>,
    retrieved_spans: Vec<Option<Vec<crate::retrieved_content::RetrievedSpan>>>,
    authored_id: Option<String>,
}

impl ModelContent {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            blocks: vec![ModelContentBlock::Text(text.into())],
            retrieved_spans: vec![Some(Vec::new())],
            prunable_tool_result: false,
            tool_success: None,
            authored_id: None,
        }
    }

    pub fn multimodal(text: impl Into<String>, images: &[ModelImage]) -> Self {
        let mut blocks = vec![ModelContentBlock::Text(text.into())];
        blocks.extend(images.iter().cloned().map(ModelContentBlock::Image));
        Self {
            retrieved_spans: vec![Some(Vec::new()); blocks.len()],
            blocks,
            prunable_tool_result: false,
            tool_success: None,
            authored_id: None,
        }
    }

    pub fn image_from_tool(call_id: &str, image: ModelImage) -> Self {
        Self {
            retrieved_spans: vec![Some(Vec::new()); 2],
            prunable_tool_result: false,
            tool_success: Some(true),
            authored_id: None,
            blocks: vec![
                ModelContentBlock::Text(format!("Image returned by view_image call {call_id}.")),
                ModelContentBlock::Image(image),
            ],
        }
    }

    /// Annotate source bytes, keeping the canonical/local text untouched.
    pub(crate) fn with_authored_id(mut self, id: String) -> Self {
        self.authored_id = Some(id);
        self
    }
    pub(crate) fn authored_id(&self) -> Option<&str> {
        self.authored_id.as_deref()
    }
    pub(crate) fn authored_text(&self) -> Result<String, String> {
        let mut output = String::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if let ModelContentBlock::Text(text) = block {
                let mut offset = 0;
                for span in self
                    .retrieved_spans
                    .get(index)
                    .and_then(Option::as_deref)
                    .unwrap_or_default()
                {
                    if span.start < offset
                        || span.end > text.len()
                        || !text.is_char_boundary(span.start)
                        || !text.is_char_boundary(span.end)
                    {
                        return Err("private authored ranges are invalid".into());
                    }
                    output.push_str(&text[offset..span.start]);
                    offset = span.end;
                }
                output.push_str(&text[offset..]);
                output.push('\n');
            }
        }
        Ok(output.trim_end_matches('\n').into())
    }
    pub(crate) fn project_authored(
        &self,
        strip_sources: bool,
        project: impl Fn(&str) -> String,
    ) -> Result<Self, String> {
        let mut next = self.clone();
        next.blocks.clear();
        next.retrieved_spans.clear();
        for (index, block) in self.blocks.iter().enumerate() {
            match block {
                ModelContentBlock::Image(_) if strip_sources => {}
                ModelContentBlock::Image(image) => {
                    next.blocks.push(ModelContentBlock::Image(image.clone()));
                    next.retrieved_spans.push(Some(Vec::new()));
                }
                ModelContentBlock::Text(text) => {
                    let mut output = String::new();
                    let mut spans = Vec::new();
                    let mut offset = 0;
                    for span in self
                        .retrieved_spans
                        .get(index)
                        .and_then(Option::as_deref)
                        .unwrap_or_default()
                    {
                        if span.start < offset
                            || span.end > text.len()
                            || !text.is_char_boundary(span.start)
                            || !text.is_char_boundary(span.end)
                        {
                            return Err("private authored ranges are invalid".into());
                        }
                        output.push_str(&project(&text[offset..span.start]));
                        if !strip_sources {
                            let start = output.len();
                            output.push_str(&text[span.start..span.end]);
                            spans.push(crate::retrieved_content::RetrievedSpan {
                                source: span.source.clone(),
                                start,
                                end: output.len(),
                            });
                        }
                        offset = span.end;
                    }
                    output.push_str(&project(&text[offset..]));
                    next.blocks.push(ModelContentBlock::Text(output));
                    next.retrieved_spans.push(Some(spans));
                }
            }
        }
        Ok(next)
    }

    /// Annotate source bytes, keeping the canonical/local text untouched.
    pub fn with_retrieved_spans(
        mut self,
        spans: Vec<crate::retrieved_content::RetrievedSpan>,
    ) -> Self {
        if let Some(metadata) = self.retrieved_spans.first_mut() {
            *metadata = Some(spans);
        }
        self
    }

    pub(crate) fn with_source_receipt(mut self, source: Option<String>) -> Self {
        if let (Some(source), Some(ModelContentBlock::Text(text)), Some(metadata)) = (
            source,
            self.blocks.first(),
            self.retrieved_spans.first_mut(),
        ) {
            *metadata = Some(vec![crate::retrieved_content::RetrievedSpan {
                start: 0,
                end: text.len(),
                source: Some(source),
            }]);
        }
        self
    }

    pub(crate) fn text_retrieved_spans(
        &self,
    ) -> Option<Vec<crate::retrieved_content::RetrievedSpan>> {
        self.as_str()?;
        self.retrieved_spans.first().cloned().flatten()
    }

    pub(crate) fn source_receipt(&self) -> Option<String> {
        let text = self.as_str()?;
        let spans = self.retrieved_spans.first()?.as_ref()?;
        let [span] = spans.as_slice() else {
            return None;
        };
        (span.start == 0 && span.end == text.len())
            .then(|| span.source.clone())
            .flatten()
    }

    /// Append host-authored directions without dropping source annotations.
    /// Existing spans remain subject to validation during request projection.
    pub(crate) fn append_authored(&self, suffix: &str) -> Self {
        let mut content = self.clone();
        if let Some(text) = content.as_text_mut() {
            text.push_str(suffix);
        }
        content
    }

    pub fn with_legacy_provenance(mut self) -> Self {
        self.retrieved_spans.fill(None);
        self
    }

    pub fn project_retrieved(
        &self,
        filter: &crate::provider_filter::ProviderFilter,
    ) -> Result<Self, crate::agent::stream::ProviderError> {
        let mut projected = self.clone();
        for (index, block) in projected.blocks.iter_mut().enumerate() {
            match block {
                ModelContentBlock::Text(text) => {
                    let metadata = self.retrieved_spans.get(index).and_then(Option::as_deref);
                    let spans = crate::retrieved_content::checked_spans(
                        text,
                        metadata,
                        filter.substitutes_retrieved(),
                    )?;
                    let mut result = String::new();
                    let mut offset = 0;
                    for span in spans {
                        filter.admit_outbound(&text.as_bytes()[span.start..span.end])?;
                        result.push_str(&text[offset..span.start]);
                        result.push_str(&filter.encode_retrieved(&text[span.start..span.end]));
                        offset = span.end;
                    }
                    result.push_str(&text[offset..]);
                    *text = result;
                }
                ModelContentBlock::Image(_) if filter.replaces_images() => {
                    *block = ModelContentBlock::Text("[image]".into());
                }
                ModelContentBlock::Image(_) => {}
            }
        }
        projected.retrieved_spans.fill(Some(Vec::new()));
        Ok(projected)
    }

    /// A strict projection consumes host-owned source ranges before adapter
    /// lowering. Unclassified tool payloads are entirely project-origin data.
    pub(crate) fn project_privacy(
        &self,
        context: &crate::privacy::Context,
        tool_result: bool,
    ) -> Result<Self, String> {
        let mut projected = self.clone();
        for (index, block) in projected.blocks.iter_mut().enumerate() {
            match block {
                ModelContentBlock::Image(_)
                    if context.policy().default_action != crate::privacy::policy::Action::Allow =>
                {
                    *block = ModelContentBlock::Text("[image]".into())
                }
                ModelContentBlock::Image(_) => {}
                ModelContentBlock::Text(text) => {
                    if tool_result {
                        let metadata = self.retrieved_spans.get(index).and_then(Option::as_deref);
                        let spans =
                            crate::retrieved_content::checked_spans(text, metadata, true)
                                .map_err(|_| "Privacy source ranges are unavailable or invalid")?;
                        let mut result = String::new();
                        let mut offset = 0;
                        for span in spans {
                            if offset < span.start {
                                result.push_str(
                                    &context.project_retrieved(&text[offset..span.start], None)?,
                                );
                            }
                            result.push_str(&context.project_retrieved(
                                &text[span.start..span.end],
                                span.source.as_deref(),
                            )?);
                            offset = span.end;
                        }
                        if offset < text.len() {
                            result.push_str(&context.project_retrieved(&text[offset..], None)?);
                        }
                        *text = result;
                        if self.tool_success == Some(false) {
                            text.insert_str(0, "[tool failed]\n");
                        }
                    } else {
                        let metadata = self.retrieved_spans.get(index).and_then(Option::as_deref);
                        let spans =
                            crate::retrieved_content::checked_spans(text, metadata, true)
                                .map_err(|_| "Privacy source ranges are unavailable or invalid")?;
                        let mut result = String::new();
                        let mut offset = 0;
                        for span in spans {
                            result.push_str(&text[offset..span.start]);
                            result.push_str(&context.project_retrieved(
                                &text[span.start..span.end],
                                span.source.as_deref(),
                            )?);
                            offset = span.end;
                        }
                        result.push_str(&text[offset..]);
                        *text = result;
                    }
                }
            }
        }
        projected.retrieved_spans.fill(Some(Vec::new()));
        Ok(projected)
    }

    pub(crate) fn mask_privacy(&self, context: &crate::privacy::Context) -> Result<Self, String> {
        let mut projected = self.clone();
        for block in &mut projected.blocks {
            if let ModelContentBlock::Text(text) = block {
                *text = context.mask_known(text)?;
            }
        }
        Ok(projected)
    }

    pub(crate) fn with_prunable_tool_result(mut self, eligible: bool) -> Self {
        self.prunable_tool_result = eligible;
        self
    }
    pub(crate) fn with_tool_result_status(mut self, success: bool) -> Self {
        self.tool_success = Some(success);
        self
    }
    pub(crate) fn tool_result_success(&self) -> Option<bool> {
        self.tool_success
    }
    pub(crate) fn is_prunable_tool_result(&self) -> bool {
        self.prunable_tool_result
    }

    pub fn as_str(&self) -> Option<&str> {
        match self.blocks.as_slice() {
            [ModelContentBlock::Text(text)] => Some(text),
            _ => None,
        }
    }

    pub fn as_text_mut(&mut self) -> Option<&mut String> {
        match self.blocks.as_mut_slice() {
            [ModelContentBlock::Text(text)] => Some(text),
            _ => None,
        }
    }

    pub fn blocks(&self) -> &[ModelContentBlock] {
        &self.blocks
    }

    pub fn has_images(&self) -> bool {
        self.blocks
            .iter()
            .any(|block| matches!(block, ModelContentBlock::Image(_)))
    }

    pub fn safe_snapshot_value(&self) -> serde_json::Value {
        if let Some(text) = self.as_str() {
            return serde_json::Value::String(text.to_string());
        }
        serde_json::Value::Array(
            self.blocks
                .iter()
                .map(|block| match block {
                    ModelContentBlock::Text(text) => {
                        serde_json::json!({"type":"text", "text":text})
                    }
                    ModelContentBlock::Image(image) => serde_json::json!({
                        "type":"image",
                        "media_type":image.media_type(),
                        "width":image.width(),
                        "height":image.height(),
                        "payload":"[binary content omitted]",
                    }),
                })
                .collect(),
        )
    }
}

impl Serialize for ModelContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(text) = self.as_str() {
            return serializer.serialize_str(text);
        }
        let mut sequence = serializer.serialize_seq(Some(self.blocks.len()))?;
        for block in &self.blocks {
            sequence.serialize_element(&SafeBlock(block))?;
        }
        sequence.end()
    }
}

struct SafeBlock<'a>(&'a ModelContentBlock);

impl Serialize for SafeBlock<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            ModelContentBlock::Text(text) => {
                let mut state = serializer.serialize_struct("Text", 2)?;
                state.serialize_field("type", "text")?;
                state.serialize_field("text", text)?;
                state.end()
            }
            ModelContentBlock::Image(image) => {
                let mut state = serializer.serialize_struct("Image", 5)?;
                state.serialize_field("type", "image")?;
                state.serialize_field("media_type", image.media_type())?;
                state.serialize_field("width", &image.width())?;
                state.serialize_field("height", &image.height())?;
                state.serialize_field("payload", "[binary content omitted]")?;
                state.end()
            }
        }
    }
}

pub fn normalize_encoded_image(bytes: Vec<u8>) -> Result<ModelImage, String> {
    if bytes.is_empty() {
        return Err("image is empty".into());
    }
    if bytes.len() > MAX_COMPRESSED_IMAGE_BYTES {
        return Err(format!(
            "image exceeds the {MAX_COMPRESSED_IMAGE_BYTES}-byte compressed limit"
        ));
    }
    let format = image::guess_format(&bytes).map_err(|_| "unsupported image format")?;
    if !matches!(
        format,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
            | image::ImageFormat::Bmp
            | image::ImageFormat::Tiff
    ) {
        return Err("unsupported image format".into());
    }
    let dimensions = image::ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .map_err(|_| "image is malformed or unsupported")?;
    validate_dimensions(dimensions.0, dimensions.1)?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(dimensions.0);
    limits.max_image_height = Some(dimensions.1);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| "image is malformed, unsupported, or exceeds decoded limits")?;
    normalize_dynamic_image(decoded)
}

pub fn normalize_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<ModelImage, String> {
    validate_dimensions(width, height)?;
    let expected = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    if expected > MAX_DECODED_IMAGE_BYTES || rgba.len() as u64 != expected {
        return Err("clipboard image has invalid or oversized pixel data".into());
    }
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "clipboard image has invalid pixel data".to_string())?;
    normalize_dynamic_image(image::DynamicImage::ImageRgba8(image))
}

pub fn strip_persistence_markers(mut text: String, images: &[ModelImage]) -> String {
    for image in images.iter().rev() {
        let suffix = format!("\n\n{}", image.unavailable_marker());
        if text.ends_with(&suffix) {
            text.truncate(text.len().saturating_sub(suffix.len()));
        }
    }
    text
}

/// Persistence markers form a removable suffix. Validate provenance against
/// the original text before clipping ranges to the remaining live prefix.
pub(crate) fn strip_persistence_markers_with_spans(
    text: String,
    images: &[ModelImage],
    spans: &[crate::retrieved_content::RetrievedSpan],
) -> Result<
    (String, Vec<crate::retrieved_content::RetrievedSpan>),
    crate::agent::stream::ProviderError,
> {
    crate::retrieved_content::validate_spans(&text, spans)?;
    let text = strip_persistence_markers(text, images);
    let spans = spans
        .iter()
        .filter_map(|span| {
            let end = span.end.min(text.len());
            (span.start < end).then_some(crate::retrieved_content::RetrievedSpan {
                source: (end == span.end).then(|| span.source.clone()).flatten(),
                start: span.start,
                end,
            })
        })
        .collect();
    Ok((text, spans))
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0 || height == 0 {
        return Err("image dimensions must be nonzero".into());
    }
    if pixels > MAX_IMAGE_PIXELS || pixels.saturating_mul(4) > MAX_DECODED_IMAGE_BYTES {
        return Err("image exceeds decoded pixel limits".into());
    }
    Ok(())
}

fn normalize_dynamic_image(image: image::DynamicImage) -> Result<ModelImage, String> {
    let width = image.width();
    let height = image.height();
    validate_dimensions(width, height)?;
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| "image could not be safely re-encoded")?;
    if png.len() > MAX_NORMALIZED_IMAGE_BYTES {
        return Err(format!(
            "normalized image exceeds the {MAX_NORMALIZED_IMAGE_BYTES}-byte limit"
        ));
    }
    Ok(ModelImage {
        png: png.into(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieved_projection_keeps_authored_text_and_the_original_content() {
        let filter = crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                page_stand_in: crate::provider_filter::PageStandIn::Latin,
                ..Default::default()
            },
        )
        .unwrap();
        let raw = "Read this: SecretCanary then explain.";
        let start = raw.find("SecretCanary").unwrap();
        let content = ModelContent::text(raw).with_retrieved_spans(vec![
            crate::retrieved_content::RetrievedSpan {
                source: None,
                start,
                end: start + "SecretCanary".len(),
            },
        ]);
        let projected = content.project_retrieved(&filter).unwrap();
        assert_eq!(content.as_str(), Some(raw));
        let sent = projected.as_str().unwrap();
        assert!(sent.starts_with("Read this: "));
        assert!(sent.ends_with(" then explain."));
        assert!(!sent.contains("SecretCanary"));
        assert_eq!(projected.project_retrieved(&filter).unwrap(), projected);
        let extended = content.append_authored(" Authored suffix.");
        let extended_projection = extended.project_retrieved(&filter).unwrap();
        assert!(extended_projection
            .as_str()
            .unwrap()
            .ends_with(" Authored suffix."));
        assert!(!extended_projection
            .as_str()
            .unwrap()
            .contains("SecretCanary"));
        assert_eq!(content.as_str(), Some(raw));
        let legacy = ModelContent::text("look\n\n--- Attached file: a ---\nbody\n--- End file ---")
            .with_legacy_provenance();
        assert!(legacy.project_retrieved(&filter).is_err());
        assert!(ModelContent::text(raw)
            .with_retrieved_spans(vec![crate::retrieved_content::RetrievedSpan {
                source: None,
                start: 1,
                end: usize::MAX
            }])
            .project_retrieved(&filter)
            .is_err());
    }

    #[test]
    fn retrieved_projection_replaces_images_when_substitution_is_active() {
        let image = RawClipboardImage::new(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .normalize()
            .unwrap();
        let content = ModelContent::multimodal("authored prompt", &[image]);
        let filter = crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                page_stand_in: crate::provider_filter::PageStandIn::Latin,
                ..Default::default()
            },
        )
        .unwrap();
        let projected = content.project_retrieved(&filter).unwrap();
        assert!(!projected.has_images());
        assert!(content.has_images());
        assert_eq!(
            projected.blocks(),
            &[
                ModelContentBlock::Text("authored prompt".into()),
                ModelContentBlock::Text("[image]".into())
            ]
        );
        let disabled =
            crate::provider_filter::ProviderFilter::compile(&Default::default()).unwrap();
        assert_eq!(content.project_retrieved(&disabled).unwrap(), content);
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320_u32 & (0_u32.wrapping_sub(crc & 1)));
            }
        }
        !crc
    }

    fn png_with_text_metadata(image: &ModelImage, marker: &[u8]) -> Vec<u8> {
        let insertion = 8 + 4 + 4 + 13 + 4;
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(marker.len() as u32).to_be_bytes());
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(marker);
        chunk.extend_from_slice(&crc32(&[b"tEXt".as_slice(), marker].concat()).to_be_bytes());
        let mut encoded = image.png[..insertion].to_vec();
        encoded.extend_from_slice(&chunk);
        encoded.extend_from_slice(&image.png[insertion..]);
        encoded
    }

    #[test]
    fn normalization_is_bounded_metadata_free_and_debug_safe() {
        let image = normalize_rgba(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert!(image.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let debug = format!("{image:?}");
        assert!(debug.contains("[OMITTED]"));
        assert!(!debug.contains(&image.base64()));

        let marker = b"comment\0MM067-METADATA-SENTINEL";
        let normalized = normalize_encoded_image(png_with_text_metadata(&image, marker)).unwrap();
        assert!(!normalized
            .png
            .windows(marker.len())
            .any(|window| window == marker));
    }

    #[test]
    fn invalid_rgba_and_dimensions_fail_before_encoding() {
        assert!(normalize_rgba(0, 1, Vec::new()).is_err());
        assert!(normalize_rgba(2, 2, vec![0; 3]).is_err());
        assert!(normalize_rgba(10_000, 10_000, Vec::new()).is_err());
        assert!(normalize_encoded_image(vec![0; MAX_COMPRESSED_IMAGE_BYTES + 1]).is_err());
    }

    #[test]
    fn typed_content_serialization_omits_image_bytes() {
        let image = normalize_rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let content = ModelContent::multimodal("inspect", std::slice::from_ref(&image));
        let serialized = serde_json::to_string(&content).unwrap();
        assert!(serialized.contains("binary content omitted"));
        assert!(!serialized.contains(&image.base64()));
        assert_eq!(ModelContent::text("hello").as_str(), Some("hello"));
    }

    #[test]
    fn persistence_markers_are_removed_only_from_the_live_provider_copy() {
        let first = normalize_rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let second = normalize_rgba(2, 1, vec![4, 5, 6, 255, 7, 8, 9, 255]).unwrap();
        let canonical = format!(
            "inspect\n\n{}\n\n{}",
            first.unavailable_marker(),
            second.unavailable_marker()
        );
        assert_eq!(
            strip_persistence_markers(canonical.clone(), &[first, second]),
            "inspect"
        );
        assert!(canonical.contains("unavailable after this process"));
    }

    #[test]
    fn image_marker_removal_clips_only_valid_suffix_provenance() {
        let image = normalize_rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let canonical = format!("inspect SOURCE\n\n{}", image.unavailable_marker());
        let source = crate::retrieved_content::RetrievedSpan {
            source: None,
            start: 8,
            end: 14,
        };
        let marker = crate::retrieved_content::RetrievedSpan {
            source: None,
            start: 14,
            end: canonical.len(),
        };
        let (live, spans) = strip_persistence_markers_with_spans(
            canonical.clone(),
            std::slice::from_ref(&image),
            &[source.clone(), marker],
        )
        .unwrap();
        assert_eq!(live, "inspect SOURCE");
        assert_eq!(spans, [source]);
        let (_, crossing) = strip_persistence_markers_with_spans(
            canonical.clone(),
            std::slice::from_ref(&image),
            &[crate::retrieved_content::RetrievedSpan {
                source: None,
                start: 8,
                end: canonical.len(),
            }],
        )
        .unwrap();
        assert_eq!(
            crossing,
            [crate::retrieved_content::RetrievedSpan {
                source: None,
                start: 8,
                end: 14
            }]
        );
        assert!(strip_persistence_markers_with_spans(
            canonical.clone(),
            &[image],
            &[crate::retrieved_content::RetrievedSpan {
                source: None,
                start: 8,
                end: canonical.len() + 1
            }],
        )
        .is_err());
        assert!(canonical.contains("unavailable after this process"));
    }

    #[test]
    fn allowlisted_encoded_formats_normalize_to_metadata_free_png() {
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([40, 50, 60, 255]),
        ));
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::WebP,
            image::ImageFormat::Gif,
            image::ImageFormat::Bmp,
            image::ImageFormat::Tiff,
        ] {
            let mut encoded = Vec::new();
            source
                .write_to(&mut Cursor::new(&mut encoded), format)
                .unwrap_or_else(|error| panic!("could not encode {format:?}: {error}"));
            let normalized = normalize_encoded_image(encoded).unwrap();
            assert_eq!((normalized.width(), normalized.height()), (2, 2));
            assert_eq!(normalized.media_type(), "image/png");
            assert!(normalized.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }
}
