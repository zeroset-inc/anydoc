//! Caller-selected retention policy for embedded assets.

/// Application budgets for retaining embedded asset payloads.
///
/// These budgets are distinct from AnyDoc's fixed safety limits. Crossing a
/// configured budget omits the asset while preserving document text and
/// omission provenance; crossing a fixed safety limit still fails parsing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssetRetentionPolicy {
    /// Maximum number of unique embedded assets to retain.
    pub max_unique_assets: Option<usize>,
    /// Maximum bytes to retain across all unique embedded assets.
    pub max_total_bytes: Option<usize>,
    /// Maximum bytes to retain for one embedded asset.
    pub max_asset_bytes: Option<usize>,
}
