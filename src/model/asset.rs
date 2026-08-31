/// Index into `Document::assets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssetId(pub usize);

/// Caller-selected reason that an embedded asset was not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetOmissionReason {
    /// The caller's unique-asset budget was exhausted.
    MaxUniqueAssets,
    /// The asset exceeded the caller's individual-size budget.
    MaxAssetBytes,
    /// Retaining the asset would exceed the caller's aggregate-size budget.
    MaxTotalBytes,
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
    /// The payload exactly as stored in the source, or empty when omitted.
    pub bytes: Vec<u8>,
    /// Caller-policy reason the payload is empty, distinct from a real
    /// zero-byte embedded asset.
    pub omission_reason: Option<AssetOmissionReason>,
}
