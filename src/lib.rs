//! anydoc converts documents to GitHub-Flavored Markdown.
//!
//! Recovery and skipped-content events are reported through the [`log`]
//! facade (debug/warn level); logging never changes conversion behavior and
//! its messages are not a stable API.

#![warn(missing_docs)]

pub mod assets;
pub mod model;
pub mod spreadsheet;

mod error;
mod formats;
mod package;
mod render;
mod shared;

pub use assets::AssetRetentionPolicy;
pub use error::ConvertError;
pub use render::markdown::{
    RenderedDocument, RenderedPart, render_document, render_document_parts,
};

use render::markdown::document_to_markdown;

use std::path::Path;

/// Input format. Selects the parser; container variants that share a parser
/// (docm, xlsm, ...) map onto these via [`Format::from_bytes`] or
/// [`Format::from_extension`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Binary Word 97-2003 (`.doc`).
    Doc,
    /// WordprocessingML (`.docx`, `.docm`), both Transitional and Strict.
    Docx,
    /// OpenDocument Text (`.odt`).
    Odt,
    /// Converted with [pdf-inspector], which emits Markdown directly:
    /// [`to_document`] is unsupported for PDFs. Scanned/image-only PDFs
    /// (needing OCR) error as unsupported.
    ///
    /// [pdf-inspector]: https://github.com/firecrawl/pdf-inspector
    Pdf,
    /// Binary PowerPoint 97-2003 (`.ppt`, `.pps`, `.pot`).
    Ppt,
    /// PresentationML (`.pptx`, `.pptm`, `.ppsx`, `.ppsm`).
    Pptx,
    /// Rich Text Format (`.rtf`).
    Rtf,
    /// EPUB 2 and 3 (`.epub`).
    Epub,
    /// Excel workbooks in every container calamine reads: `.xlsx`, `.xlsm`,
    /// `.xlsb`, and binary `.xls`.
    Excel,
    /// OpenDocument Spreadsheet (`.ods`).
    Ods,
    /// OpenDocument Presentation (`.odp`).
    Odp,
    /// Delimiter-separated text (`.csv`). Carries no signature, so it has to
    /// be named rather than detected.
    Csv,
}

impl Format {
    /// Detect the format from the content itself: the signature and identity
    /// each container specification designates (PDF header, RTF open group,
    /// OLE stream names, ZIP package mimetype/content types). Plain-text
    /// formats (CSV) carry no signature and return `None`; so does anything
    /// unrecognized.
    pub fn from_bytes(bytes: &[u8]) -> Option<Format> {
        formats::detect::from_bytes(bytes)
    }

    /// The format a bare extension names (no leading dot), matched
    /// case-insensitively. `None` for anything unrecognized.
    pub fn from_extension(ext: &str) -> Option<Format> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "doc" => Format::Doc,
            "docx" | "docm" => Format::Docx,
            "odt" => Format::Odt,
            "pdf" => Format::Pdf,
            "pptx" | "pptm" | "ppsx" | "ppsm" => Format::Pptx,
            "ppt" | "pps" | "pot" => Format::Ppt,
            "rtf" => Format::Rtf,
            "epub" => Format::Epub,
            "xlsx" | "xlsm" | "xlsb" | "xls" => Format::Excel,
            "ods" => Format::Ods,
            "odp" => Format::Odp,
            "csv" => Format::Csv,
            _ => return None,
        })
    }

    /// The format a path's extension names. `None` when the path has no
    /// extension or names nothing recognized.
    pub fn from_path(path: &Path) -> Option<Format> {
        path.extension().and_then(|e| e.to_str()).and_then(Format::from_extension)
    }
}

/// Convert a document file to Markdown. The format is detected from the
/// file content ([`Format::from_bytes`]); the extension is the fallback for
/// signature-less formats (CSV) and unrecognizable containers.
pub fn to_markdown(path: impl AsRef<Path>) -> Result<String, ConvertError> {
    to_markdown_with_asset_policy(path, AssetRetentionPolicy::default())
}

/// Convert a document file to Markdown under caller-selected asset budgets.
pub fn to_markdown_with_asset_policy(
    path: impl AsRef<Path>,
    asset_policy: AssetRetentionPolicy,
) -> Result<String, ConvertError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let Some(format) = Format::from_bytes(&bytes).or_else(|| Format::from_path(path)) else {
        return Err(ConvertError::Unsupported(format!(
            "unrecognized file content and extension: {}",
            path.display()
        )));
    };
    to_markdown_bytes_with_asset_policy(&bytes, format, asset_policy)
}

/// Convert an in-memory document to Markdown. Pass a [`Format`] to select the
/// parser, or `None` to detect it from the content ([`Format::from_bytes`]),
/// which signature-less formats (CSV) have to name explicitly.
pub fn to_markdown_bytes(
    bytes: &[u8],
    format: impl Into<Option<Format>>,
) -> Result<String, ConvertError> {
    to_markdown_bytes_with_asset_policy(bytes, format, AssetRetentionPolicy::default())
}

/// Convert an in-memory document to Markdown under caller-selected embedded
/// asset budgets.
pub fn to_markdown_bytes_with_asset_policy(
    bytes: &[u8],
    format: impl Into<Option<Format>>,
    asset_policy: AssetRetentionPolicy,
) -> Result<String, ConvertError> {
    let format = resolve_format(bytes, format.into())?;
    // PDFs convert to Markdown directly (pdf-inspector) without passing
    // through the document model.
    if format == Format::Pdf {
        return formats::pdf::to_markdown(bytes);
    }
    Ok(document_to_markdown(&to_document_with_asset_policy(bytes, format, asset_policy)?))
}

/// Parse an in-memory document into the document model. Pass a [`Format`] to
/// select the parser, or `None` to detect it from the content.
///
/// Unsupported for [`Format::Pdf`]: PDF conversion produces Markdown
/// directly and has no document-model form; use [`to_markdown_bytes`].
pub fn to_document(
    bytes: &[u8],
    format: impl Into<Option<Format>>,
) -> Result<model::Document, ConvertError> {
    to_document_with_asset_policy(bytes, format, AssetRetentionPolicy::default())
}

/// Parse an in-memory document with caller-selected embedded-asset budgets.
/// Text and source provenance remain available when an asset is omitted.
pub fn to_document_with_asset_policy(
    bytes: &[u8],
    format: impl Into<Option<Format>>,
    asset_policy: AssetRetentionPolicy,
) -> Result<model::Document, ConvertError> {
    formats::parse_with_asset_policy(bytes, resolve_format(bytes, format.into())?, asset_policy)
}

/// Extract spreadsheet sheet provenance and embedded DrawingML assets without
/// parsing cells or rendering tables and Markdown.
///
/// OOXML XLSX/XLSM packages are traversed through the same bounded package
/// layer as full conversion. Binary XLS/XLSB return an explicit unsupported
/// manifest because their drawing ownership is unavailable without the cell
/// parser.
pub fn extract_spreadsheet_assets(
    bytes: &[u8],
) -> Result<spreadsheet::SpreadsheetAssetManifest, ConvertError> {
    extract_spreadsheet_assets_with_policy(bytes, AssetRetentionPolicy::default())
}

/// Extract spreadsheet provenance and embedded DrawingML assets under
/// caller-selected retention budgets.
pub fn extract_spreadsheet_assets_with_policy(
    bytes: &[u8],
    asset_policy: AssetRetentionPolicy,
) -> Result<spreadsheet::SpreadsheetAssetManifest, ConvertError> {
    match Format::from_bytes(bytes) {
        Some(Format::Excel) => formats::sheet::extract_assets_with_policy(bytes, asset_policy),
        None if bytes.starts_with(b"PK\x03\x04") => {
            formats::sheet::extract_assets_with_policy(bytes, asset_policy)
        }
        _ => Err(ConvertError::Unsupported(
            "input is not a recognized Excel spreadsheet".to_string(),
        )),
    }
}

fn resolve_format(bytes: &[u8], format: Option<Format>) -> Result<Format, ConvertError> {
    format.or_else(|| Format::from_bytes(bytes)).ok_or_else(|| {
        ConvertError::Unsupported("unrecognized file content: name the format explicitly".into())
    })
}
