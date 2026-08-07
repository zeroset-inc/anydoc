//! Compact spreadsheet asset manifest bindings.

use pyo3::prelude::*;
use pyo3::types::PyList;

use anydoc::spreadsheet;

#[pyclass(frozen, get_all, module = "anydoc")]
pub struct SpreadsheetAssetManifest {
    /// available or unsupported.
    availability: &'static str,
    /// Stable machine-readable explanation when unsupported.
    reason: Option<String>,
    /// list[SpreadsheetAssetSourceUnit]
    source_units: Py<PyList>,
    /// list[Asset]
    assets: Py<PyList>,
}

#[pyclass(frozen, get_all, module = "anydoc")]
pub struct SpreadsheetAssetSourceUnit {
    /// One-based position in workbook order.
    ordinal: usize,
    /// Workbook-defined sheet name, when readable.
    name: Option<String>,
    /// complete or degraded.
    status: &'static str,
    /// Stable machine-readable explanation when degraded.
    reason: Option<String>,
    /// Distinct asset ids referenced by this sheet.
    asset_ids: Py<PyList>,
}

pub fn manifest(
    py: Python<'_>,
    manifest: spreadsheet::SpreadsheetAssetManifest,
) -> PyResult<SpreadsheetAssetManifest> {
    let source_units = manifest.source_units.into_iter().map(|unit| {
        Ok(SpreadsheetAssetSourceUnit {
            ordinal: unit.ordinal,
            name: unit.name,
            status: match unit.status {
                spreadsheet::SpreadsheetAssetUnitStatus::Complete => "complete",
                spreadsheet::SpreadsheetAssetUnitStatus::Degraded => "degraded",
            },
            reason: unit.reason,
            asset_ids: PyList::new(py, unit.asset_ids.into_iter().map(|id| id.0))?.unbind(),
        })
    });
    Ok(SpreadsheetAssetManifest {
        availability: match manifest.availability {
            spreadsheet::SpreadsheetAssetAvailability::Available => "available",
            spreadsheet::SpreadsheetAssetAvailability::Unsupported => "unsupported",
        },
        reason: manifest.reason,
        source_units: crate::document::pylist(py, source_units)?,
        assets: crate::document::pylist(
            py,
            manifest.assets.into_iter().map(|asset| crate::document::asset(py, asset)),
        )?,
    })
}
