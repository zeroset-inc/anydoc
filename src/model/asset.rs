/// Index into `Document::assets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssetId(pub usize);

/// Caller-selected reason that an embedded asset was not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetOmissionReason {
    /// The caller's embedded-image occurrence budget was exhausted.
    MaxAssetOccurrences,
    /// The caller's unique-asset budget was exhausted.
    MaxUniqueAssets,
    /// The asset exceeded the caller's individual-size budget.
    MaxAssetBytes,
    /// Retaining the asset would exceed the caller's aggregate-size budget.
    MaxTotalBytes,
    /// Retaining the image would exceed the cumulative decoded-pixel budget.
    MaxDecodedPixels,
    /// The image exceeds the caller's per-image pixel budget.
    MaxImagePixels,
    /// The image is smaller than the caller's minimum useful dimensions.
    MinImageDimension,
}

/// An embedded binary asset (image, object payload). Retained bytes make the
/// asset self-contained; caller policy can omit selected payloads while
/// preserving their identity and provenance.
#[derive(Debug, Clone)]
pub struct Asset {
    /// This asset's own index, so a detached `Asset` still identifies itself.
    pub id: AssetId,
    /// MIME type, e.g. `image/png`.
    pub media_type: String,
    /// Package part or stream the asset came from, for provenance.
    pub origin_part: String,
    /// Payload size reported by the container or observed by the parser.
    pub byte_len: usize,
    /// Original raster width, when the encoded format exposes dimensions.
    pub image_width: Option<usize>,
    /// Original raster height, when the encoded format exposes dimensions.
    pub image_height: Option<usize>,
    /// Width of the bounded decode target; never exceeds the configured edge.
    pub decode_width: Option<usize>,
    /// Height of the bounded decode target; never exceeds the configured edge.
    pub decode_height: Option<usize>,
    /// Original raster pixel count; retained images are charged once.
    pub decoded_pixels: Option<u64>,
    /// The payload exactly as stored in the source, or empty when omitted.
    pub bytes: Vec<u8>,
    /// Caller-policy reason the payload is empty, distinct from a real
    /// zero-byte embedded asset.
    pub omission_reason: Option<AssetOmissionReason>,
}
