//! Embedded-asset accumulation with fixed safety and caller retention limits.

use crate::AssetRetentionPolicy;
use crate::error::ConvertError;
use crate::model::{Asset, AssetId, AssetOmissionReason, ImageSource};
use crate::package::Package;
use crate::package::archive::AssetPart;
use crate::package::limits;
use crate::package::relationships::{Relationships, TargetMode, rel_target_path};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreparedAsset {
    Known(AssetId),
    Load { application_cap: Option<(usize, AssetOmissionReason)> },
}

pub struct AssetSink {
    pub assets: Vec<Asset>,
    /// Origin part -> asset, so repeated references share retained bytes or
    /// one omission record and never consume a budget twice.
    by_part: HashMap<String, AssetId>,
    retained_count: usize,
    retained_bytes: usize,
    policy: AssetRetentionPolicy,
}

impl Default for AssetSink {
    fn default() -> Self {
        Self::with_policy(AssetRetentionPolicy::default())
    }
}

impl AssetSink {
    pub fn with_policy(policy: AssetRetentionPolicy) -> Self {
        Self {
            assets: Vec::new(),
            by_part: HashMap::new(),
            retained_count: 0,
            retained_bytes: 0,
            policy,
        }
    }

    /// Resolve deduplication and caller limits before retaining a payload.
    /// Package callers pass an inexact size and enforce byte limits through a
    /// bounded read; direct binary-format callers pass the exact decoded size.
    fn prepare(
        &mut self,
        media_type: String,
        origin_part: String,
        byte_len: usize,
        size_is_exact: bool,
    ) -> Result<PreparedAsset, ConvertError> {
        if let Some(&id) = self.by_part.get(&origin_part) {
            return Ok(PreparedAsset::Known(id));
        }
        let reason = self.omission_reason(byte_len, size_is_exact);
        if let Some(reason) = reason {
            let id = self.insert(media_type, origin_part, byte_len, Vec::new(), Some(reason));
            return Ok(PreparedAsset::Known(id));
        }
        Ok(PreparedAsset::Load { application_cap: self.application_cap() })
    }

    /// Retain an already loaded asset or record its policy omission. Fixed
    /// safety-limit crossings remain hard errors under the default policy.
    pub fn add(
        &mut self,
        media_type: String,
        origin_part: String,
        bytes: &[u8],
    ) -> Result<AssetId, ConvertError> {
        match self.prepare(media_type.clone(), origin_part.clone(), bytes.len(), true)? {
            PreparedAsset::Known(id) => Ok(id),
            PreparedAsset::Load { .. } => {
                self.charge_retained(bytes.len())?;
                Ok(self.insert(media_type, origin_part, bytes.len(), bytes.to_vec(), None))
            }
        }
    }

    fn add_owned(
        &mut self,
        media_type: String,
        origin_part: String,
        bytes: Vec<u8>,
    ) -> Result<AssetId, ConvertError> {
        match self.prepare(media_type.clone(), origin_part.clone(), bytes.len(), true)? {
            PreparedAsset::Known(id) => Ok(id),
            PreparedAsset::Load { .. } => {
                self.charge_retained(bytes.len())?;
                Ok(self.insert(media_type, origin_part, bytes.len(), bytes, None))
            }
        }
    }

    fn omission_reason(&self, byte_len: usize, size_is_exact: bool) -> Option<AssetOmissionReason> {
        if self.policy.max_unique_assets.is_some_and(|limit| self.retained_count >= limit) {
            return Some(AssetOmissionReason::MaxUniqueAssets);
        }
        if size_is_exact && self.policy.max_asset_bytes.is_some_and(|limit| byte_len > limit) {
            return Some(AssetOmissionReason::MaxAssetBytes);
        }
        if size_is_exact
            && self.policy.max_total_bytes.is_some_and(|limit| {
                self.retained_bytes.checked_add(byte_len).is_none_or(|total| total > limit)
            })
        {
            return Some(AssetOmissionReason::MaxTotalBytes);
        }
        None
    }

    fn charge_retained(&mut self, byte_len: usize) -> Result<(), ConvertError> {
        let total = self.retained_bytes.checked_add(byte_len).ok_or_else(|| {
            ConvertError::ResourceLimit {
                limit: "max_asset_total_bytes",
                detail: "embedded asset byte count overflowed".into(),
            }
        })?;
        if total > limits::MAX_ASSET_TOTAL_BYTES {
            return Err(ConvertError::ResourceLimit {
                limit: "max_asset_total_bytes",
                detail: "embedded assets exceed the retained-bytes cap".into(),
            });
        }
        self.retained_count += 1;
        self.retained_bytes = total;
        Ok(())
    }

    fn application_cap(&self) -> Option<(usize, AssetOmissionReason)> {
        let per_asset =
            self.policy.max_asset_bytes.map(|limit| (limit, AssetOmissionReason::MaxAssetBytes));
        let aggregate = self.policy.max_total_bytes.map(|limit| {
            (limit.saturating_sub(self.retained_bytes), AssetOmissionReason::MaxTotalBytes)
        });
        match (per_asset, aggregate) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(cap), None) | (None, Some(cap)) => Some(cap),
            (None, None) => None,
        }
    }

    fn omit_loaded(
        &mut self,
        media_type: String,
        origin_part: String,
        byte_len: usize,
        reason: AssetOmissionReason,
    ) -> AssetId {
        self.insert(media_type, origin_part, byte_len, Vec::new(), Some(reason))
    }

    fn insert(
        &mut self,
        media_type: String,
        origin_part: String,
        byte_len: usize,
        bytes: Vec<u8>,
        omission_reason: Option<AssetOmissionReason>,
    ) -> AssetId {
        let id = AssetId(self.assets.len());
        self.by_part.insert(origin_part.clone(), id);
        self.assets.push(Asset { id, media_type, origin_part, byte_len, bytes, omission_reason });
        id
    }
}

/// Load a package asset under retention policy, preserving a stable asset id
/// even when its payload is omitted.
pub fn package_asset(
    pkg: &RefCell<Package<'_>>,
    assets: &RefCell<AssetSink>,
    media_type: String,
    origin_part: String,
) -> Result<Option<AssetId>, ConvertError> {
    let declared_size = match pkg.borrow_mut().part_size(&origin_part) {
        Ok(Some(size)) => size,
        Ok(None) => return Ok(None),
        Err(e) if e.is_fatal() => return Err(e),
        Err(e) => {
            log::warn!("skipping unreadable part {origin_part}: {e}");
            return Ok(None);
        }
    };
    if declared_size > limits::MAX_ENTRY_BYTES {
        return Err(ConvertError::ResourceLimit {
            limit: "max_entry_bytes",
            detail: format!("{origin_part} declares {declared_size} decompressed bytes"),
        });
    }
    let declared_size = usize::try_from(declared_size).unwrap_or(usize::MAX);
    let prepared = assets.borrow_mut().prepare(
        media_type.clone(),
        origin_part.clone(),
        declared_size,
        false,
    )?;
    match prepared {
        PreparedAsset::Known(id) => Ok(Some(id)),
        PreparedAsset::Load { application_cap } => {
            let loaded =
                pkg.borrow_mut().asset_part(&origin_part, application_cap.map(|(cap, _)| cap))?;
            match loaded {
                AssetPart::Loaded(bytes) => {
                    Ok(Some(assets.borrow_mut().add_owned(media_type, origin_part, bytes)?))
                }
                AssetPart::PolicyExceeded(observed) => {
                    let (_, reason) = application_cap.expect("bounded read has an application cap");
                    let id = assets.borrow_mut().omit_loaded(
                        media_type,
                        origin_part,
                        declared_size.max(observed),
                        reason,
                    );
                    Ok(Some(id))
                }
                AssetPart::Missing => {
                    log::warn!("asset part {origin_part} is missing");
                    Ok(None)
                }
            }
        }
    }
}

/// Resolve an image relationship to its source. External targets keep their
/// URL; internal targets are retained or omitted under caller policy.
pub fn rel_image_source(
    pkg: &RefCell<Package>,
    rels: &Relationships,
    base_part: &str,
    assets: &RefCell<AssetSink>,
    rel_id: &str,
) -> Result<Option<ImageSource>, ConvertError> {
    let Some(rel) = rels.get(rel_id) else {
        return Ok(None);
    };
    if rel.mode == TargetMode::External {
        return Ok((!rel.target.is_empty()).then(|| ImageSource::External(rel.target.clone())));
    }
    let Some(part) = rel_target_path(rels, base_part, rel_id)? else {
        return Ok(None);
    };
    let media = media_type_for(&part);
    Ok(package_asset(pkg, assets, media, part)?.map(ImageSource::Asset))
}

/// Retain or omit an internal relationship target with an explicit media
/// type, for embedded objects whose package extension is not authoritative.
pub fn rel_asset_source(
    pkg: &RefCell<Package>,
    rels: &Relationships,
    base_part: &str,
    assets: &RefCell<AssetSink>,
    rel_id: &str,
    media_type: &str,
) -> Result<Option<ImageSource>, ConvertError> {
    let Some(part) = rel_target_path(rels, base_part, rel_id)? else {
        return Ok(None);
    };
    Ok(package_asset(pkg, assets, media_type.to_string(), part)?.map(ImageSource::Asset))
}

/// MIME type from a part path's extension.
pub fn media_type_for(part: &str) -> String {
    let ext = part.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/emf",
        "wmf" => "image/wmf",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn retained(asset: &Asset) -> bool {
        asset.omission_reason.is_none()
    }

    #[test]
    fn repeated_origin_parts_share_one_asset_and_budget_charge() {
        let mut sink = AssetSink::with_policy(AssetRetentionPolicy {
            max_unique_assets: Some(1),
            max_total_bytes: Some(64),
            max_asset_bytes: None,
        });
        let a = sink.add("image/png".into(), "media/one.png".into(), &[1; 64]).unwrap();
        let b = sink.add("image/png".into(), "media/one.png".into(), &[1; 64]).unwrap();
        assert_eq!(a, b);
        assert_eq!(sink.assets.len(), 1);
        assert!(retained(&sink.assets[0]));
    }

    #[test]
    fn each_application_budget_records_a_structured_omission() {
        let cases = [
            (
                AssetRetentionPolicy { max_unique_assets: Some(0), ..Default::default() },
                AssetOmissionReason::MaxUniqueAssets,
            ),
            (
                AssetRetentionPolicy { max_asset_bytes: Some(3), ..Default::default() },
                AssetOmissionReason::MaxAssetBytes,
            ),
            (
                AssetRetentionPolicy { max_total_bytes: Some(3), ..Default::default() },
                AssetOmissionReason::MaxTotalBytes,
            ),
        ];
        for (policy, reason) in cases {
            let mut sink = AssetSink::with_policy(policy);
            let id = sink.add("image/png".into(), "media/a.png".into(), &[1; 4]).unwrap();
            assert_eq!(id, AssetId(0));
            assert_eq!(sink.assets[0].omission_reason, Some(reason));
            assert_eq!(sink.assets[0].byte_len, 4);
        }
    }

    #[test]
    fn omitted_references_deduplicate_without_consuming_retention_budget() {
        let mut sink = AssetSink::with_policy(AssetRetentionPolicy {
            max_asset_bytes: Some(3),
            ..Default::default()
        });
        let a = sink.add("image/png".into(), "media/a.png".into(), &[1; 4]).unwrap();
        let b = sink.add("image/png".into(), "media/a.png".into(), &[1; 4]).unwrap();
        let c = sink.add("image/png".into(), "media/b.png".into(), &[1; 3]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(sink.assets.len(), 2);
        assert!(retained(&sink.assets[1]));
    }

    #[test]
    fn default_policy_keeps_the_fixed_total_cap_as_a_hard_error() {
        let mut sink =
            AssetSink { retained_bytes: limits::MAX_ASSET_TOTAL_BYTES - 10, ..Default::default() };
        let err = sink.add("image/png".into(), "media/big.png".into(), &[0; 11]).unwrap_err();
        assert!(matches!(err, ConvertError::ResourceLimit { limit: "max_asset_total_bytes", .. }));
    }

    #[test]
    fn exhausted_count_budget_omits_before_reading_the_payload() {
        let payload = b"payload-that-must-not-be-read";
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "media/a.png",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(payload).unwrap();
        let mut bytes = writer.finish().unwrap().into_inner();
        let offset = bytes.windows(payload.len()).position(|window| window == payload).unwrap();
        bytes[offset] ^= 0xff; // CRC now fails if Package reads the entry.

        let pkg = RefCell::new(Package::open(&bytes).unwrap());
        let sink = RefCell::new(AssetSink::with_policy(AssetRetentionPolicy {
            max_unique_assets: Some(0),
            ..Default::default()
        }));
        let id =
            package_asset(&pkg, &sink, "image/png".into(), "media/a.png".into()).unwrap().unwrap();

        assert_eq!(id, AssetId(0));
        assert_eq!(
            sink.borrow().assets[0].omission_reason,
            Some(AssetOmissionReason::MaxUniqueAssets)
        );
    }
}
