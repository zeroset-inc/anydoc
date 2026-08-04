/// One source-defined unit of a document, mapped onto a half-open range of
/// top-level blocks. Units preserve boundaries that flattening into a block
/// stream would otherwise lose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceUnit {
    /// The source structure this unit represents.
    pub kind: SourceUnitKind,
    /// 1-based position in the source, including units that could not be
    /// read or contained no blocks.
    pub ordinal: usize,
    /// Source-defined name, when the unit has one (for example, a sheet
    /// name). Presentation slides generally leave this unset.
    pub name: Option<String>,
    /// Whether this unit was read and produced content.
    pub status: SourceUnitStatus,
    /// Stable machine-readable explanation when [`Self::status`] is
    /// [`SourceUnitStatus::Skipped`].
    pub reason: Option<String>,
    /// Index of the first top-level block in [`crate::model::Document::blocks`].
    pub start_block: usize,
    /// Exclusive index after the last top-level block in this unit.
    pub end_block: usize,
}

/// Source structures that divide a document into ordered units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUnitKind {
    /// A presentation slide.
    Slide,
    /// A spreadsheet sheet.
    Sheet,
}

/// Extraction outcome for one source unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUnitStatus {
    /// The unit was read and contributed at least one block.
    Parsed,
    /// The unit was read successfully but contained no extractable blocks.
    Empty,
    /// Some or all of the unit could not be read.
    Skipped,
}
