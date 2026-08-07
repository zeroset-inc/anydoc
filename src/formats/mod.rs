//! One frontend per input format; each parses bytes into the document model.

mod csv;
pub mod detect;
mod doc;
mod docx;
mod epub;
mod odf;
pub mod pdf;
mod ppt;
mod pptx;
mod rtf;
pub(crate) mod sheet;

use crate::Format;
use crate::error::ConvertError;
use crate::model::Document;

pub fn parse_with_asset_policy(
    bytes: &[u8],
    format: Format,
    asset_policy: crate::AssetRetentionPolicy,
) -> Result<Document, ConvertError> {
    match format {
        Format::Excel => sheet::parse_with_asset_policy(bytes, asset_policy),
        Format::Csv => csv::parse(bytes),
        Format::Docx => docx::parse_with_asset_policy(bytes, asset_policy),
        Format::Odt | Format::Ods | Format::Odp => {
            odf::parse_with_asset_policy(bytes, asset_policy)
        }
        Format::Pptx => pptx::parse_with_asset_policy(bytes, asset_policy),
        Format::Epub => epub::parse_with_asset_policy(bytes, asset_policy),
        Format::Rtf => rtf::parse_with_asset_policy(bytes, asset_policy),
        // RTF files wearing a .doc extension are common in the wild.
        Format::Doc if bytes.starts_with(b"{\\rtf") => {
            rtf::parse_with_asset_policy(bytes, asset_policy)
        }
        Format::Doc => doc::parse_with_asset_policy(bytes, asset_policy),
        Format::Ppt => ppt::parse_with_asset_policy(bytes, asset_policy),
        // pdf-inspector produces Markdown directly; there is no document
        // model for PDFs. `to_markdown_bytes` routes them to `pdf`.
        Format::Pdf => Err(ConvertError::Unsupported(
            "PDF converts directly to Markdown; use to_markdown or to_markdown_bytes".to_string(),
        )),
    }
}
