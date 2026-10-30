//! Embedded-asset retention policy bindings.

use pyo3::prelude::*;

/// Application budgets for embedded asset payload retention.
#[pyclass(frozen, get_all, module = "anydoc", skip_from_py_object)]
#[derive(Clone)]
pub struct AssetRetentionPolicy {
    /// Maximum number of embedded-image references to retain.
    max_asset_occurrences: Option<usize>,
    /// Maximum number of unique payloads to retain.
    max_unique_assets: Option<usize>,
    /// Maximum retained bytes across all unique payloads.
    max_total_bytes: Option<usize>,
    /// Maximum retained bytes for one payload.
    max_asset_bytes: Option<usize>,
    /// Maximum original raster pixels across retained images.
    max_decoded_pixels: Option<u64>,
    /// Maximum original raster pixels for one image.
    max_image_pixels: Option<u64>,
    /// Maximum longest edge exposed as the bounded decode target.
    max_image_edge: Option<usize>,
    /// Minimum width and height for retained raster images.
    min_image_dimension: Option<usize>,
}

#[pymethods]
impl AssetRetentionPolicy {
    #[new]
    #[pyo3(signature = (*, max_asset_occurrences=None, max_unique_assets=None, max_total_bytes=None, max_asset_bytes=None, max_decoded_pixels=None, max_image_pixels=None, max_image_edge=None, min_image_dimension=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        max_asset_occurrences: Option<usize>,
        max_unique_assets: Option<usize>,
        max_total_bytes: Option<usize>,
        max_asset_bytes: Option<usize>,
        max_decoded_pixels: Option<u64>,
        max_image_pixels: Option<u64>,
        max_image_edge: Option<usize>,
        min_image_dimension: Option<usize>,
    ) -> Self {
        Self {
            max_asset_occurrences,
            max_unique_assets,
            max_total_bytes,
            max_asset_bytes,
            max_decoded_pixels,
            max_image_pixels,
            max_image_edge,
            min_image_dimension,
        }
    }

    /// Return Nebula's reviewed bounded Office-image policy.
    #[staticmethod]
    fn office_safe() -> Self {
        let policy = anydoc::AssetRetentionPolicy::office_safe();
        Self {
            max_asset_occurrences: policy.max_asset_occurrences,
            max_unique_assets: policy.max_unique_assets,
            max_total_bytes: policy.max_total_bytes,
            max_asset_bytes: policy.max_asset_bytes,
            max_decoded_pixels: policy.max_decoded_pixels,
            max_image_pixels: policy.max_image_pixels,
            max_image_edge: policy.max_image_edge,
            min_image_dimension: policy.min_image_dimension,
        }
    }
}

impl AssetRetentionPolicy {
    pub fn core(&self) -> anydoc::AssetRetentionPolicy {
        anydoc::AssetRetentionPolicy {
            max_asset_occurrences: self.max_asset_occurrences,
            max_unique_assets: self.max_unique_assets,
            max_total_bytes: self.max_total_bytes,
            max_asset_bytes: self.max_asset_bytes,
            max_decoded_pixels: self.max_decoded_pixels,
            max_image_pixels: self.max_image_pixels,
            max_image_edge: self.max_image_edge,
            min_image_dimension: self.min_image_dimension,
        }
    }
}

pub fn core(policy: Option<PyRef<'_, AssetRetentionPolicy>>) -> anydoc::AssetRetentionPolicy {
    policy.as_deref().map(AssetRetentionPolicy::core).unwrap_or_default()
}
