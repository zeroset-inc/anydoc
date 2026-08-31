use anydoc::model::{SourceUnitKind, SourceUnitStatus};
use anydoc::{AssetRetentionPolicy, Format};
use std::io::{Cursor, Write};

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{path}")).unwrap()
}

#[test]
fn presentation_and_spreadsheet_units_are_ordered_and_rendered_as_parts() {
    for (path, format, kind) in [
        ("ppt/pres.ppt", Format::Ppt, SourceUnitKind::Slide),
        ("pptx/pres.pptx", Format::Pptx, SourceUnitKind::Slide),
        ("xlsx/sheet.xlsx", Format::Excel, SourceUnitKind::Sheet),
        ("xls/sheet.xls", Format::Excel, SourceUnitKind::Sheet),
        ("xlsb/handmade-sheet.xlsb", Format::Excel, SourceUnitKind::Sheet),
    ] {
        let document = anydoc::to_document(&fixture(path), format).unwrap();
        assert!(!document.source_units.is_empty(), "{path}");
        let mut end = 0;
        for (index, unit) in document.source_units.iter().enumerate() {
            assert_eq!(unit.kind, kind, "{path}");
            assert_eq!(unit.ordinal, index + 1, "{path}");
            assert!(unit.start_block >= end && unit.end_block >= unit.start_block, "{path}");
            assert!(unit.end_block <= document.blocks.len(), "{path}");
            end = unit.end_block;
        }
        let parts = anydoc::render_document_parts(&document);
        for index in 0..document.source_units.len() {
            assert!(parts.iter().any(|part| part.source_unit_index == Some(index)), "{path}");
        }
    }
}

#[test]
fn xlsx_images_keep_sheet_ownership_and_policy_omissions() {
    let bytes = image_workbook();
    let manifest = anydoc::extract_spreadsheet_assets(&bytes).unwrap();
    assert_eq!(manifest.source_units.len(), 2);
    assert_eq!(manifest.source_units[0].name.as_deref(), Some("Empty"));
    assert!(manifest.source_units[0].asset_ids.is_empty());
    assert_eq!(manifest.source_units[1].name.as_deref(), Some("Pictures"));
    assert_eq!(manifest.source_units[1].asset_ids.len(), 1);
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].bytes, b"owned-image");

    let document = anydoc::to_document(&bytes, Format::Excel).unwrap();
    assert_eq!(document.source_units.len(), 2);
    assert_eq!(document.source_units[0].status, SourceUnitStatus::Empty);
    assert_eq!(document.source_units[1].status, SourceUnitStatus::Parsed);
    let parts = anydoc::render_document_parts(&document);
    assert_eq!(parts[1].source_unit_index, Some(1));
    assert_eq!(parts[1].asset_ids, vec![anydoc::model::AssetId(0)]);

    let omitted = anydoc::extract_spreadsheet_assets_with_policy(
        &bytes,
        AssetRetentionPolicy { max_unique_assets: Some(0), ..Default::default() },
    )
    .unwrap();
    assert!(omitted.assets[0].bytes.is_empty());
    assert_eq!(
        omitted.assets[0].omission_reason,
        Some(anydoc::model::AssetOmissionReason::MaxUniqueAssets)
    );
    assert_eq!(omitted.source_units[1].asset_ids, vec![anydoc::model::AssetId(0)]);
}

#[test]
fn repeated_asset_references_remain_owned_by_each_source_unit() {
    use anydoc::model::{Asset, AssetId, Block, Document, ImageSource, Inline, SourceUnit};

    let image = || {
        Block::Paragraph(vec![Inline::Image {
            alt: "shared".into(),
            source: ImageSource::Asset(AssetId(0)),
        }])
    };
    let document = Document {
        blocks: vec![image(), image()],
        source_units: vec![
            SourceUnit {
                kind: SourceUnitKind::Slide,
                ordinal: 1,
                name: None,
                status: SourceUnitStatus::Parsed,
                reason: None,
                start_block: 0,
                end_block: 1,
            },
            SourceUnit {
                kind: SourceUnitKind::Slide,
                ordinal: 2,
                name: None,
                status: SourceUnitStatus::Parsed,
                reason: None,
                start_block: 1,
                end_block: 2,
            },
        ],
        notes: Vec::new(),
        assets: vec![Asset {
            id: AssetId(0),
            media_type: "image/png".into(),
            origin_part: "ppt/media/shared.png".into(),
            byte_len: 3,
            image_width: Some(100),
            image_height: Some(100),
            decode_width: Some(100),
            decode_height: Some(100),
            decoded_pixels: Some(10_000),
            bytes: vec![1, 2, 3],
            omission_reason: None,
        }],
    };

    let parts = anydoc::render_document_parts(&document);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].asset_ids, vec![AssetId(0)]);
    assert_eq!(parts[1].asset_ids, vec![AssetId(0)]);
}

fn image_workbook() -> Vec<u8> {
    let parts = [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Empty" sheetId="1" r:id="rId1"/><sheet name="Pictures" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rIdDrawing"/></worksheet>"#,
        ),
        (
            "xl/worksheets/_rels/sheet2.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDrawing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#,
        ),
        (
            "xl/drawings/drawing1.xml",
            r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="1" cy="1"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="1" name="Owned"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rIdImage"/></xdr:blipFill></xdr:pic></xdr:absoluteAnchor></xdr:wsDr>"#,
        ),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/owned.png"/></Relationships>"#,
        ),
    ];
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.start_file("xl/media/owned.png", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"owned-image").unwrap();
    zip.finish().unwrap().into_inner()
}
