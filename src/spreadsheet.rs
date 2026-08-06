//! Compact, bounded spreadsheet asset extraction.

use crate::model::{Asset, AssetId};

/// Whether DrawingML asset extraction is available for this spreadsheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadsheetAssetAvailability {
    /// The OOXML workbook and its sheet relationships were inspected.
    Available,
    /// Binary XLS/XLSB drawings are not supported without cell parsing.
    Unsupported,
}

/// Completeness of one sheet's DrawingML relationship traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadsheetAssetUnitStatus {
    /// Every drawing relationship that was found was inspected.
    Complete,
    /// At least one drawing relationship or target was unreadable.
    Degraded,
}

/// Embedded assets owned by one workbook sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpreadsheetAssetSourceUnit {
    /// One-based position in workbook order.
    pub ordinal: usize,
    /// Workbook-defined sheet name, when readable.
    pub name: Option<String>,
    /// Completeness of DrawingML extraction for this sheet.
    pub status: SpreadsheetAssetUnitStatus,
    /// Stable machine-readable reason when extraction degraded.
    pub reason: Option<String>,
    /// Distinct retained assets referenced by this sheet, in first-use order.
    pub asset_ids: Vec<AssetId>,
}

/// Compact spreadsheet source-unit and embedded-asset manifest.
#[derive(Debug, Clone)]
pub struct SpreadsheetAssetManifest {
    /// Whether this container exposes bounded OOXML DrawingML assets.
    pub availability: SpreadsheetAssetAvailability,
    /// Stable machine-readable explanation when extraction is unsupported.
    pub reason: Option<String>,
    /// Every OOXML sheet in workbook order, including sheets with no images.
    pub source_units: Vec<SpreadsheetAssetSourceUnit>,
    /// Retained embedded assets, indexed by their ids.
    pub assets: Vec<Asset>,
}
