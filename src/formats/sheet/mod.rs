//! Excel spreadsheets (xlsx, xlsm, xlsb, xls) via calamine.

mod images;

use crate::error::ConvertError;
use crate::model::{
    Block, Cell, CellSlot, Document, GridBuilder, Inline, SourceUnit, SourceUnitKind,
    SourceUnitStatus, Table, TableKind,
};
use crate::shared::header::resolve_header_rows;
use crate::shared::text::clean_text;
use calamine::{Data, Dimensions, Reader, Sheets, open_workbook_auto_from_rs};
use images::{SheetImage, xlsx_images};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

/// Run one calamine operation behind a panic barrier: the calamine fork can
/// panic on corrupt containers (pending an upstream fix), and a dependency
/// panic must degrade to a typed error - while bugs in this crate's own code
/// stay panics. `AssertUnwindSafe` is sound here because a caught panic
/// always propagates as an error, so the workbook is never used again.
fn contained<T>(op: &str, f: impl FnOnce() -> T) -> Result<T, ConvertError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|_| {
        log::warn!("spreadsheet parser panicked during {op} on malformed input");
        ConvertError::malformed("unreadable workbook (parser aborted)")
    })
}

pub fn parse(bytes: &[u8]) -> Result<Document, ConvertError> {
    let mut workbook =
        contained("workbook open", || open_workbook_auto_from_rs(Cursor::new(bytes)))?
            .map_err(map_open_error)?;
    let sheet_names = contained("sheet listing", || workbook.sheet_names().to_owned())?;
    let multi_sheet = sheet_names.len() > 1;
    let merged = merged_regions(&mut workbook, &sheet_names)?;
    // Calamine's picture feature eagerly decompresses and clones every image
    // before AnyDoc can enforce archive and retained-asset limits. Parse OOXML
    // drawings through the bounded package layer instead. Binary XLS/XLSB do
    // not expose reliable image positions and remain cell-text-only.
    let extracted_images = xlsx_images(bytes)?;
    let mut images = extracted_images.by_sheet;
    let mut degraded_drawings = extracted_images.degraded_sheets;
    let assets = extracted_images.assets;

    let mut doc = Document::default();
    let mut failed = 0usize;
    for (sheet_index, name) in sheet_names.iter().enumerate() {
        let start_block = doc.blocks.len();
        let sheet_images = images.remove(name).unwrap_or_default();
        let drawing_degraded = degraded_drawings.remove(name);
        let range = match contained("worksheet read", || workbook.worksheet_range(name))? {
            Ok(r) => r,
            Err(e) => {
                log::warn!("skipping unreadable sheet {name:?}: {e}");
                let has_images = !sheet_images.is_empty();
                if !has_images {
                    failed += 1;
                } else {
                    append_images(&mut doc.blocks, sheet_images);
                }
                doc.source_units.push(SourceUnit {
                    kind: SourceUnitKind::Sheet,
                    ordinal: sheet_index + 1,
                    name: Some(name.clone()),
                    status: SourceUnitStatus::Skipped,
                    reason: Some("worksheet_unreadable".to_string()),
                    start_block,
                    end_block: doc.blocks.len(),
                });
                continue;
            }
        };
        // Merged regions in range-relative coordinates: the top-left cell
        // becomes a spanning origin, the other positions are covered.
        let start = range.start().unwrap_or((0, 0));
        let (height, width) = (range.height(), range.width());
        let mut origins: HashMap<(usize, usize), (u32, u32)> = HashMap::new();
        let mut covered: HashSet<(usize, usize)> = HashSet::new();
        for d in merged.get(name.as_str()).map(Vec::as_slice).unwrap_or_default() {
            // Intersect the absolute merged region with the used range first:
            // a region wholly above or left of the range must not saturate
            // onto relative (0,0), and positions outside the range are never
            // materialized (a crafted region list must not force insertions
            // beyond the cells that actually exist).
            let (row0, col0) = (d.start.0.max(start.0), d.start.1.max(start.1));
            let row_end = (d.end.0 as u64 + 1).min(start.0 as u64 + height as u64);
            let col_end = (d.end.1 as u64 + 1).min(start.1 as u64 + width as u64);
            if (row0 as u64) >= row_end || (col0 as u64) >= col_end {
                continue;
            }
            // Translate the non-empty intersection to range-relative form.
            let r0 = (row0 - start.0) as usize;
            let c0 = (col0 - start.1) as usize;
            let r1 = (row_end - start.0 as u64) as usize;
            let c1 = (col_end - start.1 as u64) as usize;
            if r1 - r0 == 1 && c1 - c0 == 1 {
                continue;
            }
            origins.insert((r0, c0), ((c1 - c0) as u32, (r1 - r0) as u32));
            for r in r0..r1 {
                for c in c0..c1 {
                    if (r, c) != (r0, c0) {
                        covered.insert((r, c));
                    }
                }
            }
        }
        let mut builder = GridBuilder::new();
        if !range.is_empty() {
            for (r, row) in range.rows().enumerate() {
                builder.next_row();
                for (c, data) in row.iter().enumerate() {
                    if covered.contains(&(r, c)) {
                        builder.covered();
                        continue;
                    }
                    let text = format_data(data);
                    let cell = if text.is_empty() {
                        Cell::default()
                    } else {
                        Cell::from_inlines(vec![Inline::plain(text)])
                    };
                    match origins.get(&(r, c)) {
                        Some(&(col_span, row_span)) => {
                            builder.place(Cell::spanning(cell.blocks, col_span, row_span))?
                        }
                        None => builder.place(cell)?,
                    }
                }
            }
        }
        // A spreadsheet marks no header row, so the shape of the data decides.
        let mut table = builder.finish(TableKind::Data);
        if !table.grid.is_empty() {
            table.header_rows = resolve_header_rows(&table, 0);
        }
        let mut trailing_images = Vec::new();
        for image in sheet_images {
            if let Err(image) = place_anchored_image(&mut table, start, image) {
                trailing_images.push(image);
            }
        }
        let has_content = !table.grid.is_empty() || !trailing_images.is_empty();
        if multi_sheet && has_content {
            doc.blocks.push(Block::heading(2, vec![Inline::plain(name.clone())]));
        }
        if !table.grid.is_empty() {
            doc.blocks.push(Block::Table(table));
        }
        append_images(&mut doc.blocks, trailing_images);
        doc.source_units.push(SourceUnit {
            kind: SourceUnitKind::Sheet,
            ordinal: sheet_index + 1,
            name: Some(name.clone()),
            status: if drawing_degraded {
                SourceUnitStatus::Skipped
            } else if has_content {
                SourceUnitStatus::Parsed
            } else {
                SourceUnitStatus::Empty
            },
            reason: drawing_degraded.then(|| "worksheet_drawing_unreadable".to_string()),
            start_block,
            end_block: doc.blocks.len(),
        });
    }
    if !sheet_names.is_empty() && failed == sheet_names.len() {
        return Err(ConvertError::malformed("no sheet in the workbook could be read"));
    }
    // A valid workbook cannot normally name a drawing on an unknown sheet,
    // but retaining such content outside all sheet units is more honest than
    // assigning false provenance or dropping it.
    let mut unknown_sheets: Vec<_> = images.into_iter().collect();
    unknown_sheets.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_, images) in unknown_sheets {
        append_images(&mut doc.blocks, images);
    }
    doc.assets = assets.assets;
    Ok(doc)
}

fn append_images(blocks: &mut Vec<Block>, images: Vec<SheetImage>) {
    blocks.extend(images.into_iter().map(|image| Block::Paragraph(vec![image.inline])));
}

/// Place an image into its anchored cell when that cell already exists in the
/// bounded used range. Covered positions resolve to their merged-cell origin.
/// Returning the image leaves the caller to append it within the sheet unit,
/// without expanding a sparse table out to a distant anchor.
fn place_anchored_image(
    table: &mut Table,
    start: (u32, u32),
    image: SheetImage,
) -> Result<(), SheetImage> {
    let Some((row, col)) = image.anchor else {
        return Err(image);
    };
    let Some(row) = row.checked_sub(start.0).map(|v| v as usize) else {
        return Err(image);
    };
    let Some(col) = col.checked_sub(start.1).map(|v| v as usize) else {
        return Err(image);
    };
    let target = match table.grid.get(row).and_then(|r| r.get(col)) {
        Some(CellSlot::Origin(_)) => (row, col),
        Some(CellSlot::Covered { origin_row, origin_col }) => (*origin_row, *origin_col),
        None => return Err(image),
    };
    let Some(CellSlot::Origin(cell)) =
        table.grid.get_mut(target.0).and_then(|r| r.get_mut(target.1))
    else {
        return Err(image);
    };
    cell.blocks.push(Block::Paragraph(vec![image.inline]));
    Ok(())
}

/// Merged regions per sheet, where the container format exposes them (xlsx
/// via each worksheet's mergeCells part, xls via BIFF MERGEDCELLS).
fn merged_regions<RS: std::io::Read + std::io::Seek>(
    workbook: &mut Sheets<RS>,
    sheet_names: &[String],
) -> Result<HashMap<String, Vec<Dimensions>>, ConvertError> {
    let mut out: HashMap<String, Vec<Dimensions>> = HashMap::new();
    for name in sheet_names {
        let regions = match workbook {
            Sheets::Xlsx(x) => {
                contained("merged-region listing", || x.merge_cells_by_sheet_name(name))?
                    .map_err(|e| e.to_string())
            }
            Sheets::Xls(x) => {
                contained("merged-cell listing", || x.merge_cells_by_sheet_name(name))?
                    .map_err(|e| e.to_string())
            }
            _ => continue,
        };
        match regions {
            Ok(dims) if !dims.is_empty() => {
                out.insert(name.clone(), dims);
            }
            Ok(_) => {}
            Err(e) => log::warn!("skipping unreadable merged-region list for {name:?}: {e}"),
        }
    }
    Ok(out)
}

fn map_open_error(e: calamine::Error) -> ConvertError {
    let text = e.to_string();
    if text.to_ascii_lowercase().contains("password") {
        ConvertError::Encrypted
    } else {
        ConvertError::malformed(format!("unreadable workbook: {text}"))
    }
}

fn format_data(data: &Data) -> String {
    match data {
        Data::Empty => String::new(),
        // Untrimmed: leading/trailing whitespace in a cell is source content.
        Data::String(s) => clean_text(s),
        Data::Float(f) => format_float(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::Error(e) => format!("#{e:?}"),
        Data::DateTime(dt) if dt.is_duration() => format_duration_days(dt.as_f64()),
        // A serial below one whole day carries no date: it is a time of day.
        Data::DateTime(dt) if dt.as_f64().abs() < 1.0 => format_time_of_day(dt.as_f64()),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(d) => {
                let s = d.to_string();
                // Sub-second digits are noise from the serial's float.
                let s = s.split('.').next().unwrap_or(&s);
                s.strip_suffix(" 00:00:00").unwrap_or(s).to_string()
            }
            None => format_float(dt.as_f64()),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
    }
}

/// Float formatting at the 15 significant decimal digits a spreadsheet
/// stores and displays. Shortest round-trip formatting past that surfaces the
/// binary representation (`3554.7000000000003`); 15 digits still keeps small
/// values like 0.0000004 exact.
fn format_float(f: f64) -> String {
    match format!("{f:.14e}").parse::<f64>() {
        Ok(rounded) => format!("{rounded}"),
        Err(_) => format!("{f}"),
    }
}

/// Render a time-of-day serial (a fraction of a day) as `hh:mm:ss`.
fn format_time_of_day(days: f64) -> String {
    let total_secs = (days.abs() * 86_400.0).round() as u64 % 86_400;
    let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// Render an Excel duration (stored in days) as `[h]:mm:ss`.
fn format_duration_days(days: f64) -> String {
    let sign = if days < 0.0 { "-" } else { "" };
    let total_secs = (days.abs() * 86_400.0).round() as u64;
    let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
    format!("{sign}{h}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ImageSource;
    use std::io::{Read, Write};

    /// Minimal xlsx with a used range at D11:E12 and the given merged region.
    fn xlsx_with_merge(merge_ref: &str) -> Vec<u8> {
        let sheet = format!(
            r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="11"><c r="D11" t="inlineStr"><is><t>x</t></is></c><c r="E11" t="inlineStr"><is><t>y</t></is></c></row><row r="12"><c r="D12" t="inlineStr"><is><t>z</t></is></c><c r="E12" t="inlineStr"><is><t>w</t></is></c></row></sheetData><mergeCells count="1"><mergeCell ref="{merge_ref}"/></mergeCells></worksheet>"#
        );
        let parts: &[(&str, &str)] = &[
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
        ];
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.start_file("xl/worksheets/sheet1.xml", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(sheet.as_bytes()).unwrap();
        w.finish().unwrap().into_inner()
    }

    fn cell_anchor(kind: &str, row: u32, col: u32, name: &str) -> String {
        let extent = match kind {
            "oneCellAnchor" => r#"<xdr:ext cx="1" cy="1"/>"#.to_string(),
            "twoCellAnchor" => format!(
                r#"<xdr:to><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>"#,
                col + 1,
                row + 1
            ),
            other => panic!("unsupported anchor kind {other}"),
        };
        format!(
            r#"<xdr:{kind}><xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>{extent}<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="1" name="{name}"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rIdImage"/></xdr:blipFill><xdr:spPr/></xdr:pic><xdr:clientData/></xdr:{kind}>"#
        )
    }

    fn absolute_anchor(name: &str) -> String {
        format!(
            r#"<xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="1" cy="1"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="1" name="{name}"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rIdImage"/></xdr:blipFill><xdr:spPr/></xdr:pic><xdr:clientData/></xdr:absoluteAnchor>"#
        )
    }

    fn shape_anchor() -> &'static str {
        r#"<xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="1" cy="1"/><xdr:sp><xdr:nvSpPr><xdr:cNvPr id="1" name="Shape"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr/></xdr:sp><xdr:clientData/></xdr:oneCellAnchor>"#
    }

    /// Minimal XLSX with one worksheet drawing and one shared image part.
    fn xlsx_with_images(
        sheet_data: &str,
        merge_ref: Option<&str>,
        anchors: &str,
        image_rel_type: &str,
    ) -> Vec<u8> {
        let merge = merge_ref.map_or(String::new(), |range| {
            format!(r#"<mergeCells count="1"><mergeCell ref="{range}"/></mergeCells>"#)
        });
        let worksheet = format!(
            r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData>{sheet_data}</sheetData>{merge}<drawing r:id="rIdDrawing"/></worksheet>"#
        );
        let drawing = format!(
            r#"<?xml version="1.0"?><xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{anchors}</xdr:wsDr>"#
        );
        let drawing_rels = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="{image_rel_type}" Target="../media/image1.png"/></Relationships>"#
        );
        let parts: Vec<(&str, Vec<u8>)> = vec![
            (
                "[Content_Types].xml",
                br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/></Types>"#.to_vec(),
            ),
            (
                "_rels/.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec(),
            ),
            (
                "xl/workbook.xml",
                br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Pictures" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_vec(),
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_vec(),
            ),
            ("xl/worksheets/sheet1.xml", worksheet.into_bytes()),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDrawing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#.to_vec(),
            ),
            ("xl/drawings/drawing1.xml", drawing.into_bytes()),
            ("xl/drawings/_rels/drawing1.xml.rels", drawing_rels.into_bytes()),
            ("xl/media/image1.png", b"PNG-IMAGE-PAYLOAD".to_vec()),
        ];
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in parts {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// Replace or remove one ZIP member without changing the other fixture
    /// parts. `None` removes the member.
    fn rewrite_zip_part(bytes: &[u8], target: &str, replacement: Option<&[u8]>) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let mut found = false;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().to_string();
            if name == target {
                found = true;
                if let Some(replacement) = replacement {
                    writer.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
                    writer.write_all(replacement).unwrap();
                }
                continue;
            }
            let options =
                zip::write::SimpleFileOptions::default().compression_method(file.compression());
            writer.start_file(&name, options).unwrap();
            let mut body = Vec::new();
            file.read_to_end(&mut body).unwrap();
            writer.write_all(&body).unwrap();
        }
        assert!(found, "fixture part {target} was not found");
        writer.finish().unwrap().into_inner()
    }

    /// Raise one entry's declared size without allocating its claimed body.
    fn set_declared_size(zip: &mut [u8], part: &str, size: u32) {
        let mut updated = 0;
        let mut offset = 0;
        while offset + 46 <= zip.len() {
            let signature = &zip[offset..offset + 4];
            let (name_len_offset, name_offset, size_offset) = match signature {
                b"PK\x03\x04" => (26, 30, 22),
                b"PK\x01\x02" => (28, 46, 24),
                _ => {
                    offset += 1;
                    continue;
                }
            };
            let name_len = u16::from_le_bytes([
                zip[offset + name_len_offset],
                zip[offset + name_len_offset + 1],
            ]) as usize;
            let name_end = offset + name_offset + name_len;
            if name_end <= zip.len() && &zip[offset + name_offset..name_end] == part.as_bytes() {
                zip[offset + size_offset..offset + size_offset + 4]
                    .copy_from_slice(&size.to_le_bytes());
                updated += 1;
            }
            offset = name_end.max(offset + 1);
        }
        assert_eq!(updated, 2, "expected local and central headers for {part}");
    }

    fn image_count(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(inlines) => {
                    inlines.iter().filter(|inline| matches!(inline, Inline::Image { .. })).count()
                }
                _ => 0,
            })
            .sum()
    }

    fn image_alts(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(inlines) => Some(inlines),
                _ => None,
            })
            .flatten()
            .filter_map(|inline| match inline {
                Inline::Image { alt, .. } => Some(alt.as_str()),
                _ => None,
            })
            .collect()
    }

    fn image_sources(blocks: &[Block]) -> Vec<&ImageSource> {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(inlines) => Some(inlines),
                _ => None,
            })
            .flatten()
            .filter_map(|inline| match inline {
                Inline::Image { source, .. } => Some(source),
                _ => None,
            })
            .collect()
    }

    fn first_table(doc: &Document) -> &Table {
        doc.blocks
            .iter()
            .find_map(|block| match block {
                Block::Table(table) => Some(table),
                _ => None,
            })
            .expect("table")
    }

    fn covered_count(doc: &Document) -> usize {
        let Some(Block::Table(t)) = doc.blocks.first() else {
            panic!("expected a table, got {:?}", doc.blocks.first());
        };
        t.grid
            .iter()
            .flatten()
            .filter(|s| matches!(s, crate::model::CellSlot::Covered { .. }))
            .count()
    }

    #[test]
    fn merge_inside_the_used_range_covers_cells() {
        // Harness sanity: an in-range merge must actually load and apply.
        let doc = parse(&xlsx_with_merge("D11:E11")).unwrap();
        assert_eq!(covered_count(&doc), 1);
    }

    #[test]
    fn merge_outside_the_used_columns_is_ignored() {
        // M6: the merge overlaps the used rows but not the used columns; the
        // old relative saturation mapped it onto (0,0) and covered D12.
        let doc = parse(&xlsx_with_merge("A1:B12")).unwrap();
        assert_eq!(covered_count(&doc), 0, "out-of-range merge must not cover cells");
    }

    #[test]
    fn positioned_images_attach_to_their_cell_and_share_the_asset() {
        let sheet_data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row><row r="2"><c r="B2" t="inlineStr"><is><t>b</t></is></c></row>"#;
        let anchors = format!(
            "{}{}",
            cell_anchor("oneCellAnchor", 1, 1, "One"),
            cell_anchor("twoCellAnchor", 1, 1, "Two")
        );
        let doc = parse(&xlsx_with_images(sheet_data, None, &anchors, images::IMAGE_REL)).unwrap();

        assert_eq!(doc.assets.len(), 1, "two placements of one part must deduplicate");
        assert_eq!(doc.assets[0].origin_part, "xl/media/image1.png");
        assert_eq!(doc.assets[0].bytes, b"PNG-IMAGE-PAYLOAD");
        let table = first_table(&doc);
        let CellSlot::Origin(cell) = &table.grid[1][1] else { panic!("B2 must be an origin") };
        assert_eq!(image_count(&cell.blocks), 2);
        assert_eq!(image_alts(&cell.blocks), ["One", "Two"]);
        assert_eq!(doc.source_units[0].kind, SourceUnitKind::Sheet);
        assert_eq!(doc.source_units[0].name.as_deref(), Some("Pictures"));
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Parsed);
        assert_eq!((doc.source_units[0].start_block, doc.source_units[0].end_block), (0, 1));
    }

    #[test]
    fn merged_cell_anchor_attaches_to_the_origin() {
        let sheet_data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c><c r="B1" t="inlineStr"><is><t>b</t></is></c></row>"#;
        let anchor = cell_anchor("oneCellAnchor", 0, 1, "Merged");
        let doc = parse(&xlsx_with_images(sheet_data, Some("A1:B1"), &anchor, images::IMAGE_REL))
            .unwrap();
        let table = first_table(&doc);
        let CellSlot::Origin(cell) = &table.grid[0][0] else { panic!("A1 must be an origin") };
        assert_eq!(image_count(&cell.blocks), 1);
        assert!(matches!(table.grid[0][1], CellSlot::Covered { origin_row: 0, origin_col: 0 }));
    }

    #[test]
    fn image_only_sheet_retains_an_absolute_image_in_its_unit() {
        let doc =
            parse(&xlsx_with_images("", None, &absolute_anchor("Only image"), images::IMAGE_REL))
                .unwrap();

        assert_eq!(doc.assets.len(), 1);
        assert_eq!(image_count(&doc.blocks), 1);
        assert!(!doc.blocks.iter().any(|b| matches!(b, Block::Table(_))));
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Parsed);
        assert_eq!((doc.source_units[0].start_block, doc.source_units[0].end_block), (0, 1));
    }

    #[test]
    fn missing_drawing_part_marks_the_sheet_as_skipped() {
        let bytes =
            xlsx_with_images("", None, &absolute_anchor("Missing drawing"), images::IMAGE_REL);
        let bytes = rewrite_zip_part(&bytes, "xl/drawings/drawing1.xml", None);
        let doc = parse(&bytes).unwrap();

        assert!(doc.blocks.is_empty());
        assert!(doc.assets.is_empty());
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Skipped);
        assert_eq!(doc.source_units[0].reason.as_deref(), Some("worksheet_drawing_unreadable"));
    }

    #[test]
    fn non_picture_drawing_anchor_does_not_degrade_the_sheet() {
        let doc = parse(&xlsx_with_images("", None, shape_anchor(), images::IMAGE_REL)).unwrap();

        assert!(doc.blocks.is_empty());
        assert!(doc.assets.is_empty());
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Empty);
        assert_eq!(doc.source_units[0].reason, None);
    }

    #[test]
    fn externally_linked_image_is_retained_as_an_external_source() {
        let sheet_data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row>"#;
        let anchor = cell_anchor("oneCellAnchor", 0, 0, "Linked").replace("r:embed", "r:link");
        let bytes = xlsx_with_images(sheet_data, None, &anchor, images::IMAGE_REL);
        let external_rels = format!(
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="{}" Target="https://example.com/picture.png" TargetMode="External"/></Relationships>"#,
            images::IMAGE_REL
        );
        let bytes = rewrite_zip_part(
            &bytes,
            "xl/drawings/_rels/drawing1.xml.rels",
            Some(external_rels.as_bytes()),
        );
        let doc = parse(&bytes).unwrap();

        assert!(doc.assets.is_empty());
        let table = first_table(&doc);
        let CellSlot::Origin(cell) = &table.grid[0][0] else { panic!("A1 must be an origin") };
        assert_eq!(
            image_sources(&cell.blocks),
            [&ImageSource::External("https://example.com/picture.png".to_string())]
        );
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Parsed);
    }

    #[test]
    fn image_recovery_keeps_an_unreadable_sheet_as_skipped_content() {
        let invalid_cell =
            r#"<row r="1"><c r="not-a-cell" t="inlineStr"><is><t>bad</t></is></c></row>"#;
        let doc = parse(&xlsx_with_images(
            invalid_cell,
            None,
            &absolute_anchor("Recovered image"),
            images::IMAGE_REL,
        ))
        .unwrap();

        assert_eq!(image_count(&doc.blocks), 1);
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Skipped);
        assert_eq!(doc.source_units[0].reason.as_deref(), Some("worksheet_unreadable"));
    }

    #[test]
    fn empty_sheet_has_an_empty_source_unit() {
        let doc = parse(&xlsx_with_images("", None, "", images::IMAGE_REL)).unwrap();

        assert!(doc.blocks.is_empty());
        assert!(doc.assets.is_empty(), "an unreferenced media part is not document content");
        assert_eq!(doc.source_units.len(), 1);
        assert_eq!(doc.source_units[0].status, SourceUnitStatus::Empty);
        assert_eq!((doc.source_units[0].start_block, doc.source_units[0].end_block), (0, 0));
    }

    #[test]
    fn distant_anchor_does_not_expand_the_used_range() {
        let sheet_data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row>"#;
        let anchor = cell_anchor("oneCellAnchor", 99_999, 99_999, "Distant");
        let doc = parse(&xlsx_with_images(sheet_data, None, &anchor, images::IMAGE_REL)).unwrap();

        let table = first_table(&doc);
        assert_eq!((table.grid.len(), table.grid[0].len()), (1, 1));
        assert_eq!(image_count(&doc.blocks), 1, "out-of-range image follows the table");
        assert_eq!((doc.source_units[0].start_block, doc.source_units[0].end_block), (0, 2));
    }

    #[test]
    fn non_image_relationship_is_not_loaded_as_an_asset() {
        let sheet_data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row>"#;
        let anchor = cell_anchor("oneCellAnchor", 0, 0, "Not image data");
        let doc = parse(&xlsx_with_images(
            sheet_data,
            None,
            &anchor,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart",
        ))
        .unwrap();

        assert!(doc.assets.is_empty());
        assert_eq!(
            image_count(
                first_table(&doc).grid[0]
                    .iter()
                    .find_map(|slot| match slot {
                        CellSlot::Origin(cell) => Some(cell.blocks.as_slice()),
                        _ => None,
                    })
                    .unwrap()
            ),
            0
        );
    }

    #[test]
    fn image_package_limits_propagate() {
        let sheet_data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row>"#;
        let anchor = cell_anchor("oneCellAnchor", 0, 0, "Oversize");
        let mut bytes = xlsx_with_images(sheet_data, None, &anchor, images::IMAGE_REL);
        set_declared_size(
            &mut bytes,
            "xl/media/image1.png",
            (crate::package::limits::MAX_ENTRY_BYTES + 1) as u32,
        );

        let err = parse(&bytes).unwrap_err();
        assert!(
            matches!(err, ConvertError::ResourceLimit { limit: "max_entry_bytes", .. }),
            "expected the package limit, got {err}"
        );
    }

    #[test]
    fn string_cells_are_not_trimmed() {
        assert_eq!(format_data(&Data::String("  padded  ".into())), "  padded  ");
    }

    #[test]
    fn tiny_floats_survive() {
        assert_eq!(format_float(0.0000004), "0.0000004");
        assert_eq!(format_float(12.0), "12");
        assert_eq!(format_float(1.5), "1.5");
    }

    #[test]
    fn time_of_day_serials_carry_no_date() {
        // 09:04:54 as a fraction of a day, with the float noise a serial
        // carries in practice.
        assert_eq!(format_time_of_day(32_694.184 / 86_400.0), "09:04:54");
        assert_eq!(format_time_of_day(0.0), "00:00:00");
    }

    #[test]
    fn floats_render_at_spreadsheet_precision() {
        assert_eq!(format_float(3554.7000000000003), "3554.7");
        assert_eq!(format_float(5649.5599999999995), "5649.56");
        assert_eq!(format_float(346_289_529.491_800_1), "346289529.4918");
        // Small values stay exact: 15 significant digits reaches far below 1.
        assert_eq!(format_float(0.0000004), "0.0000004");
        assert_eq!(format_float(1.0), "1");
    }

    #[test]
    fn durations_render_as_clock_time() {
        // 26h30m15s = 1.104340277... days
        let days = (26.0 * 3600.0 + 30.0 * 60.0 + 15.0) / 86_400.0;
        assert_eq!(format_duration_days(days), "26:30:15");
        assert_eq!(format_duration_days(-0.5), "-12:00:00");
    }
}
