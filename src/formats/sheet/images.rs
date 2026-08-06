//! Bounded extraction of SpreadsheetML DrawingML pictures.

use crate::error::ConvertError;
use crate::model::{ImageSource, Inline};
use crate::package::Package;
use crate::package::path::resolve;
use crate::package::relationships::{
    RelationshipPart, Relationships, TargetMode, read_relationship_part, read_rels, rel_type,
    rels_part_for,
};
use crate::package::xml::{Element, ns};
use crate::shared::assets::{AssetSink, rel_image_source};
use crate::shared::text::clean_text;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

const SPREADSHEET_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const SPREADSHEET_DRAWING_NS: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const DRAWING_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
pub(super) const IMAGE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const WORKSHEET_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";

#[derive(Debug)]
pub(super) struct SheetImage {
    /// Absolute zero-based worksheet anchor. `None` is an absolute anchor,
    /// which belongs to the sheet but carries no honest cell coordinate.
    pub anchor: Option<(u32, u32)>,
    pub alt: String,
    pub source: ImageSource,
}

impl SheetImage {
    pub fn into_inline(self) -> Inline {
        Inline::Image { alt: self.alt, source: self.source }
    }
}

pub(super) struct XlsxImageUnit {
    pub name: Option<String>,
    pub asset_ids: Vec<crate::model::AssetId>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XlsxImageAvailability {
    Available,
    UnsupportedBinary,
    Unavailable,
}

pub(super) struct XlsxImages {
    pub availability: XlsxImageAvailability,
    pub units: Vec<XlsxImageUnit>,
    pub by_sheet: HashMap<String, Vec<SheetImage>>,
    /// Sheets whose drawing graph existed but could not be read completely.
    pub degraded_sheets: HashSet<String>,
    pub assets: AssetSink,
}

/// Extract standard DrawingML pictures from an XLSX/XLSM package. Every part
/// read passes through `Package`, so archive/XML limits apply before bytes are
/// retained by `AssetSink`. Other Calamine containers return no drawings.
pub(super) fn xlsx_images(bytes: &[u8]) -> Result<XlsxImages, ConvertError> {
    let mut units = Vec::new();
    let mut by_sheet = HashMap::new();
    let mut degraded_sheets = HashSet::new();
    let assets = RefCell::new(AssetSink::new());
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok(XlsxImages {
            availability: XlsxImageAvailability::UnsupportedBinary,
            units,
            by_sheet,
            degraded_sheets,
            assets: assets.into_inner(),
        });
    }
    let pkg = RefCell::new(Package::open(bytes)?);
    let package_rels = read_rels(&mut pkg.borrow_mut(), "_rels/.rels")?;
    let workbook_part = package_rels
        .iter()
        .filter(|(_, rel)| {
            rel.mode == TargetMode::Internal && rel.rel_type == rel_type::OFFICE_DOCUMENT
        })
        .min_by_key(|(id, _)| *id)
        .map(|(_, rel)| rel)
        .map(|rel| resolve("", &rel.target))
        .transpose();
    let workbook_part = match workbook_part {
        Ok(Some(target)) => target.path,
        Ok(None) => {
            return Ok(XlsxImages {
                availability: XlsxImageAvailability::Unavailable,
                units,
                by_sheet,
                degraded_sheets,
                assets: assets.into_inner(),
            });
        }
        Err(e) => {
            log::warn!("skipping unresolvable workbook target: {e}");
            return Ok(XlsxImages {
                availability: XlsxImageAvailability::Unavailable,
                units,
                by_sheet,
                degraded_sheets,
                assets: assets.into_inner(),
            });
        }
    };
    if !workbook_part.ends_with(".xml") {
        return Ok(XlsxImages {
            availability: XlsxImageAvailability::UnsupportedBinary,
            units,
            by_sheet,
            degraded_sheets,
            assets: assets.into_inner(),
        });
    }
    let Some(workbook) = pkg.borrow_mut().optional_xml_part(&workbook_part)? else {
        return Ok(XlsxImages {
            availability: XlsxImageAvailability::Unavailable,
            units,
            by_sheet,
            degraded_sheets,
            assets: assets.into_inner(),
        });
    };
    let workbook_rels = read_rels(&mut pkg.borrow_mut(), &rels_part_for(&workbook_part))?;
    for sheet in workbook.descendants(SPREADSHEET_NS, "sheet") {
        let name = sheet.attr(SPREADSHEET_NS, "name").map(str::to_string);
        let sheet_target = name.as_ref().and_then(|_| {
            sheet
                .attr_qualified(ns::R, "id")
                .and_then(|rel_id| workbook_rels.get(rel_id))
                .filter(|rel| rel.mode == TargetMode::Internal && rel.rel_type == WORKSHEET_REL)
                .map(|rel| resolve(&workbook_part, &rel.target))
        });
        let legacy_target = sheet_target.is_some();
        let (images, degraded) = match sheet_target {
            Some(Ok(target)) => read_sheet_images(&pkg, &target.path, &assets)?,
            Some(Err(e)) => {
                log::warn!("skipping unresolvable worksheet target: {e}");
                (Vec::new(), true)
            }
            None => (Vec::new(), true),
        };
        let mut seen = HashSet::new();
        let asset_ids = images
            .iter()
            .filter_map(|image| match image.source {
                ImageSource::Asset(id) if seen.insert(id) => Some(id),
                _ => None,
            })
            .collect();
        if let Some(name) = &name {
            if !images.is_empty() {
                by_sheet.entry(name.clone()).or_insert_with(Vec::new).extend(images);
            }
            if degraded && legacy_target {
                degraded_sheets.insert(name.clone());
            }
        }
        units.push(XlsxImageUnit { name, asset_ids, degraded });
    }
    Ok(XlsxImages {
        availability: XlsxImageAvailability::Available,
        units,
        by_sheet,
        degraded_sheets,
        assets: assets.into_inner(),
    })
}

fn read_sheet_images(
    pkg: &RefCell<Package<'_>>,
    sheet_part: &str,
    assets: &RefCell<AssetSink>,
) -> Result<(Vec<SheetImage>, bool), ConvertError> {
    let sheet_rels =
        match read_relationship_part(&mut pkg.borrow_mut(), &rels_part_for(sheet_part))? {
            RelationshipPart::Absent => return Ok((Vec::new(), false)),
            RelationshipPart::Unreadable => return Ok((Vec::new(), true)),
            RelationshipPart::Parsed(rels) => rels,
        };
    // Most sheets have no drawings. Check the small relationships part first
    // so the common path does not decompress and DOM-parse the large worksheet
    // a second time after Calamine has already read its cells.
    if !sheet_rels
        .iter()
        .any(|(_, rel)| rel.mode == TargetMode::Internal && rel.rel_type == DRAWING_REL)
    {
        return Ok((Vec::new(), false));
    }
    let Some(sheet) = pkg.borrow_mut().optional_xml_part(sheet_part)? else {
        return Ok((Vec::new(), true));
    };
    let mut out = Vec::new();
    let mut degraded = false;
    let mut seen_drawings = HashSet::new();
    for drawing in sheet.descendants(SPREADSHEET_NS, "drawing") {
        let Some(rel_id) = drawing.attr_qualified(ns::R, "id") else {
            degraded = true;
            continue;
        };
        let Some(rel) = sheet_rels.get(rel_id) else {
            degraded = true;
            continue;
        };
        if rel.mode != TargetMode::Internal || rel.rel_type != DRAWING_REL {
            degraded = true;
            continue;
        }
        let drawing_part = match resolve(sheet_part, &rel.target) {
            Ok(target) => target.path,
            Err(e) => {
                log::warn!("skipping unresolvable drawing target {:?}: {e}", rel.target);
                degraded = true;
                continue;
            }
        };
        if !seen_drawings.insert(drawing_part.clone()) {
            continue;
        }
        let Some(tree) = pkg.borrow_mut().optional_xml_part(&drawing_part)? else {
            degraded = true;
            continue;
        };
        let drawing_rels = read_rels(&mut pkg.borrow_mut(), &rels_part_for(&drawing_part))?;
        let Some(root) = tree.child_elems().find(|e| e.is(SPREADSHEET_DRAWING_NS, "wsDr")) else {
            degraded = true;
            continue;
        };
        for anchor in root.child_elems().filter(|e| {
            e.ns.as_deref() == Some(SPREADSHEET_DRAWING_NS)
                && matches!(e.local.as_str(), "oneCellAnchor" | "twoCellAnchor" | "absoluteAnchor")
        }) {
            let (picture, picture_degraded) =
                drawing_image(pkg, &drawing_part, &drawing_rels, anchor, assets)?;
            if let Some(picture) = picture {
                out.push(picture);
            }
            degraded |= picture_degraded;
        }
    }
    Ok((out, degraded))
}

fn drawing_image(
    pkg: &RefCell<Package<'_>>,
    drawing_part: &str,
    rels: &Relationships,
    anchor: &Element,
    assets: &RefCell<AssetSink>,
) -> Result<(Option<SheetImage>, bool), ConvertError> {
    let Some(picture) = anchor.first_descendant(SPREADSHEET_DRAWING_NS, "pic") else {
        // Worksheet drawings can also anchor shapes and charts. They are not
        // missing pictures and do not make image extraction incomplete.
        return Ok((None, false));
    };
    let Some(blip) = picture.first_descendant(ns::A, "blip") else {
        return Ok((None, true));
    };
    let Some(rel_id) =
        blip.attr_qualified(ns::R, "embed").or_else(|| blip.attr_qualified(ns::R, "link"))
    else {
        return Ok((None, true));
    };
    let Some(rel) = rels.get(rel_id) else {
        return Ok((None, true));
    };
    if rel.rel_type != IMAGE_REL {
        return Ok((None, true));
    }
    let props = picture.first_descendant(SPREADSHEET_DRAWING_NS, "cNvPr");
    let alt = props
        .and_then(|p| p.attr(SPREADSHEET_DRAWING_NS, "descr"))
        .filter(|s| !s.trim().is_empty())
        .or_else(|| props.and_then(|p| p.attr(SPREADSHEET_DRAWING_NS, "name")))
        .map(|s| clean_text(s.trim()))
        .unwrap_or_else(|| "Spreadsheet image".to_string());
    let source = rel_image_source(pkg, rels, drawing_part, assets, rel_id)?;
    let degraded = source.is_none();
    let source = source.unwrap_or(ImageSource::Unavailable);
    let anchor = anchor.find(SPREADSHEET_DRAWING_NS, "from").and_then(|from| {
        let row = from.find(SPREADSHEET_DRAWING_NS, "row")?.text().trim().parse().ok()?;
        let col = from.find(SPREADSHEET_DRAWING_NS, "col")?.text().trim().parse().ok()?;
        Some((row, col))
    });
    Ok((Some(SheetImage { anchor, alt, source }), degraded))
}
