//! Caller-selected retention policy for embedded assets.

/// Default maximum number of retained embedded-image occurrences.
pub const DEFAULT_MAX_ASSET_OCCURRENCES: usize = 200;
/// Default cumulative decoded-pixel budget for embedded raster images.
pub const DEFAULT_MAX_DECODED_PIXELS: u64 = 64 * 1024 * 1024;
/// Default encoded-byte budget retained across embedded assets.
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
/// Default encoded-byte budget retained for one embedded asset.
pub const DEFAULT_MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;
/// Default decoded-pixel budget for one embedded raster image.
pub const DEFAULT_MAX_IMAGE_PIXELS: u64 = 32 * 1024 * 1024;
/// Default longest edge callers may decode an embedded image to.
pub const DEFAULT_MAX_IMAGE_EDGE: usize = 1_536;
/// Default minimum width and height for a retained embedded raster image.
pub const DEFAULT_MIN_IMAGE_DIMENSION: usize = 50;

/// Application budgets for retaining embedded asset payloads.
///
/// These budgets are distinct from AnyDoc's fixed safety limits. Crossing a
/// configured count, byte, pixel, or minimum-dimension budget omits the asset
/// while preserving document text and omission provenance. The edge budget
/// instead exposes a bounded aspect-preserving decode target. Crossing a fixed
/// parser safety limit still fails parsing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssetRetentionPolicy {
    /// Maximum embedded-image references retained across the document.
    pub max_asset_occurrences: Option<usize>,
    /// Maximum number of unique embedded assets to retain.
    pub max_unique_assets: Option<usize>,
    /// Maximum bytes to retain across all unique embedded assets.
    pub max_total_bytes: Option<usize>,
    /// Maximum bytes to retain for one embedded asset.
    pub max_asset_bytes: Option<usize>,
    /// Maximum original raster pixels charged across unique retained images.
    pub max_decoded_pixels: Option<u64>,
    /// Maximum original raster pixels in one retained image.
    pub max_image_pixels: Option<u64>,
    /// Maximum longest edge exposed as the safe decode target.
    pub max_image_edge: Option<usize>,
    /// Minimum width and height for a retained raster image.
    pub min_image_dimension: Option<usize>,
}

impl AssetRetentionPolicy {
    /// The bounded policy used by Nebula's Office ingestion path.
    ///
    /// Plain conversion keeps upstream's self-contained, lossless default;
    /// callers that prepare images for downstream decoding opt into these
    /// reviewed budgets explicitly.
    pub const fn office_safe() -> Self {
        Self {
            max_asset_occurrences: Some(DEFAULT_MAX_ASSET_OCCURRENCES),
            max_unique_assets: Some(DEFAULT_MAX_ASSET_OCCURRENCES),
            max_total_bytes: Some(DEFAULT_MAX_TOTAL_BYTES),
            max_asset_bytes: Some(DEFAULT_MAX_ASSET_BYTES),
            max_decoded_pixels: Some(DEFAULT_MAX_DECODED_PIXELS),
            max_image_pixels: Some(DEFAULT_MAX_IMAGE_PIXELS),
            max_image_edge: Some(DEFAULT_MAX_IMAGE_EDGE),
            min_image_dimension: Some(DEFAULT_MIN_IMAGE_DIMENSION),
        }
    }
}
