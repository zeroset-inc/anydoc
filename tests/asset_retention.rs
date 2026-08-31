use std::path::{Path, PathBuf};

use anydoc::model::AssetOmissionReason;
use anydoc::{AssetRetentionPolicy, Format};

fn fixture(path: impl AsRef<Path>) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::read(root.join(path)).unwrap()
}

fn zero_retention() -> AssetRetentionPolicy {
    AssetRetentionPolicy { max_unique_assets: Some(0), ..Default::default() }
}

#[test]
fn docx_policy_preserves_text_ownership_and_omission_provenance() {
    let bytes = fixture("docx/handmade-rich.docx");
    let full = anydoc::to_document(&bytes, Format::Docx).unwrap();
    let omitted =
        anydoc::to_document_with_asset_policy(&bytes, Format::Docx, zero_retention()).unwrap();

    assert_eq!(anydoc::render_document(&full).markdown, anydoc::render_document(&omitted).markdown);
    assert_eq!(omitted.assets.len(), full.assets.len());
    let asset = omitted.assets.iter().find(|asset| asset.media_type == "image/png").unwrap();
    assert!(asset.bytes.is_empty());
    assert!(asset.byte_len > 0);
    assert_eq!(asset.omission_reason, Some(AssetOmissionReason::MaxUniqueAssets));
    assert!(
        anydoc::render_document_parts(&omitted)
            .iter()
            .any(|part| part.asset_ids.contains(&asset.id)),
        "the omitted asset must remain owned by its rendered part"
    );
}

#[test]
fn pptx_and_odf_package_assets_obey_the_same_policy() {
    for (path, format) in
        [("pptx/handmade-order.pptx", Format::Pptx), ("odt/text.odt", Format::Odt)]
    {
        let bytes = fixture(path);
        let document =
            anydoc::to_document_with_asset_policy(&bytes, format, zero_retention()).unwrap();
        assert!(!document.assets.is_empty(), "{path} must exercise an embedded asset");
        assert!(document.assets.iter().all(|asset| {
            asset.bytes.is_empty()
                && asset.omission_reason == Some(AssetOmissionReason::MaxUniqueAssets)
        }));
        assert!(!anydoc::render_document(&document).markdown.is_empty());
    }
}

#[test]
fn per_asset_and_aggregate_reasons_are_distinct() {
    let bytes = fixture("docx/handmade-rich.docx");
    for (policy, reason) in [
        (
            AssetRetentionPolicy { max_asset_bytes: Some(0), ..Default::default() },
            AssetOmissionReason::MaxAssetBytes,
        ),
        (
            AssetRetentionPolicy { max_total_bytes: Some(0), ..Default::default() },
            AssetOmissionReason::MaxTotalBytes,
        ),
    ] {
        let document = anydoc::to_document_with_asset_policy(&bytes, Format::Docx, policy).unwrap();
        assert!(document.assets.iter().all(|asset| asset.omission_reason == Some(reason)));
    }
}
