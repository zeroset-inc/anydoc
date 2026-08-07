//! Embedded-asset retention policy bindings.

use pyo3::prelude::*;

/// Application budgets for embedded asset payload retention.
#[pyclass(frozen, get_all, module = "anydoc", skip_from_py_object)]
#[derive(Clone)]
pub struct AssetRetentionPolicy {
    /// Maximum number of unique payloads to retain.
    max_unique_assets: Option<usize>,
    /// Maximum retained bytes across all unique payloads.
    max_total_bytes: Option<usize>,
    /// Maximum retained bytes for one payload.
    max_asset_bytes: Option<usize>,
}

#[pymethods]
impl AssetRetentionPolicy {
    #[new]
    #[pyo3(signature = (*, max_unique_assets=None, max_total_bytes=None, max_asset_bytes=None))]
    fn new(
        max_unique_assets: Option<usize>,
        max_total_bytes: Option<usize>,
        max_asset_bytes: Option<usize>,
    ) -> Self {
        Self { max_unique_assets, max_total_bytes, max_asset_bytes }
    }
}

impl AssetRetentionPolicy {
    pub fn core(&self) -> anydoc::AssetRetentionPolicy {
        anydoc::AssetRetentionPolicy {
            max_unique_assets: self.max_unique_assets,
            max_total_bytes: self.max_total_bytes,
            max_asset_bytes: self.max_asset_bytes,
        }
    }
}

pub fn core(policy: Option<PyRef<'_, AssetRetentionPolicy>>) -> anydoc::AssetRetentionPolicy {
    policy.as_deref().map(AssetRetentionPolicy::core).unwrap_or_default()
}
