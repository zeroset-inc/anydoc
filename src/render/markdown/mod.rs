//! GitHub-Flavored Markdown serializer for the document model.

mod anchors;
mod escape;
mod inline;
mod table;

#[cfg(test)]
mod tests;

use crate::model::{
    AssetId, Block, Document, Inline, List, MarkerKind, Note, TableKind, inlines_are_empty,
};
use anchors::{AnchorMap, resolve_anchors};
use escape::{EscapeOpts, InlineContext, backtick_fence, escape_text};
use inline::render_inlines;
use std::collections::{HashMap, HashSet};

/// Escape a source-derived composite marker label for literal use: control
/// characters collapse to spaces and Markdown syntax is neutralized so a
/// crafted label cannot alter document structure.
pub(crate) fn escape_marker_label(label: &str, ctx: InlineContext) -> String {
    let cleaned: String = label.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let opts = EscapeOpts {
        // List-item content re-opens block syntax after the `- ` marker.
        at_line_start: ctx == InlineContext::Block,
        trailing_active: true,
        ..Default::default()
    };
    escape_text(&cleaned, ctx, opts)
}

/// Footnote id -> rendered number, shared by all render functions.
type NoteNumbers = HashMap<String, usize>;

/// Immutable render context threaded through every render function.
pub(crate) struct Ctx {
    nums: NoteNumbers,
    anchors: AnchorMap,
}

/// Markdown and provenance for one ordered range of a rendered document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPart {
    /// Markdown for this range, without source-unit boundary comments.
    pub markdown: String,
    /// Index into [`Document::source_units`], or `None` for unowned content.
    pub source_unit_index: Option<usize>,
    /// First top-level block covered by this part.
    pub start_block: usize,
    /// Exclusive top-level block bound covered by this part.
    pub end_block: usize,
    /// Distinct embedded assets referenced below this range, in first-use order.
    /// Assets with no inline reference are assigned to a trailing unowned part.
    pub asset_ids: Vec<AssetId>,
}

/// Complete Markdown together with its ordered source-provenance parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDocument {
    /// Complete Markdown, including source-unit markers and note definitions.
    pub markdown: String,
    /// Ordered source-unit and unowned block ranges.
    pub parts: Vec<RenderedPart>,
}

pub fn document_to_markdown(doc: &Document) -> String {
    let rc = Ctx { nums: number_notes(doc), anchors: resolve_anchors(doc) };
    render_document_markdown(doc, &rc)
}

/// Render a parsed document and expose the source and asset provenance of each
/// ordered part without parsing it a second time.
pub fn render_document(doc: &Document) -> RenderedDocument {
    let rc = Ctx { nums: number_notes(doc), anchors: resolve_anchors(doc) };
    RenderedDocument {
        markdown: render_document_markdown(doc, &rc),
        parts: render_document_parts(doc, &rc),
    }
}

fn valid_source_units(doc: &Document) -> Vec<(usize, &crate::model::SourceUnit)> {
    let mut units = Vec::new();
    let mut prior_end = 0;
    for (index, unit) in doc.source_units.iter().enumerate() {
        if unit.start_block <= unit.end_block
            && unit.end_block <= doc.blocks.len()
            && unit.start_block >= prior_end
        {
            prior_end = unit.end_block;
            units.push((index, unit));
        }
    }
    units
}

fn render_document_markdown(doc: &Document, rc: &Ctx) -> String {
    let mut parts = Vec::new();
    let units = valid_source_units(doc);
    for block_index in 0..=doc.blocks.len() {
        for unit in units
            .iter()
            .map(|(_, unit)| *unit)
            .filter(|unit| unit.start_block < unit.end_block && unit.end_block == block_index)
            .rev()
        {
            parts.push(render_source_unit(unit, false));
        }
        for unit in
            units.iter().map(|(_, unit)| *unit).filter(|unit| unit.start_block == block_index)
        {
            parts.push(render_source_unit(unit, true));
            if unit.start_block == unit.end_block {
                parts.push(render_source_unit(unit, false));
            }
        }
        if let Some(block) = doc.blocks.get(block_index)
            && let Some(rendered) = render_block(block, rc)
        {
            parts.push(rendered);
        }
    }
    parts.extend(render_note_definitions(doc, rc));
    finish_markdown(parts.join("\n\n"))
}

fn render_note_definitions(doc: &Document, rc: &Ctx) -> Vec<String> {
    rendered_note_definitions(doc, rc).into_iter().map(|(_, markdown)| markdown).collect()
}

fn rendered_note_definitions<'a>(doc: &'a Document, rc: &Ctx) -> Vec<(&'a Note, String)> {
    let mut parts = Vec::new();
    let mut rendered_defs: HashSet<usize> = HashSet::new();
    let mut ordered: Vec<(&Note, usize)> =
        doc.notes.iter().filter_map(|n| rc.nums.get(&n.id).map(|&num| (n, num))).collect();
    ordered.sort_by_key(|(_, num)| *num);
    for (note, num) in ordered {
        // Duplicate note ids collapse to one number; render one definition.
        if !rendered_defs.insert(num) {
            log::debug!("duplicate note id {:?} dropped from output", note.id);
            continue;
        }
        let body = render_blocks(&note.blocks, rc);
        if body.is_empty() {
            continue;
        }
        let mut lines = body.lines();
        let first = lines.next().unwrap_or("");
        let mut s = format!("[^{num}]: {first}");
        for line in lines {
            s.push('\n');
            if !line.is_empty() {
                s.push_str("    ");
                s.push_str(line);
            }
        }
        parts.push((note, s));
    }
    parts
}

fn finish_markdown(mut out: String) -> String {
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn render_document_parts(doc: &Document, rc: &Ctx) -> Vec<RenderedPart> {
    let units = valid_source_units(doc);
    let mut parts = Vec::new();
    let mut referenced_assets = HashSet::new();
    let mut cursor = 0;

    if units.is_empty() {
        parts.push(render_part(doc, rc, None, 0, doc.blocks.len(), &mut referenced_assets));
    } else {
        for (source_unit_index, unit) in units {
            if cursor < unit.start_block {
                parts.push(render_part(
                    doc,
                    rc,
                    None,
                    cursor,
                    unit.start_block,
                    &mut referenced_assets,
                ));
            }
            parts.push(render_part(
                doc,
                rc,
                Some(source_unit_index),
                unit.start_block,
                unit.end_block,
                &mut referenced_assets,
            ));
            cursor = unit.end_block;
        }
        if cursor < doc.blocks.len() {
            parts.push(render_part(
                doc,
                rc,
                None,
                cursor,
                doc.blocks.len(),
                &mut referenced_assets,
            ));
        }
    }

    let note_definitions = rendered_note_definitions(doc, rc);
    let note_markdown = note_definitions
        .iter()
        .map(|(_, markdown)| markdown.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let note_asset_ids = collect_asset_ids_from_notes(
        note_definitions.iter().map(|(note, _)| *note),
        &mut referenced_assets,
    );
    // Assets owned by dropped empty or duplicate notes are not unreferenced
    // package assets: their content was intentionally omitted with the note.
    for note in &doc.notes {
        walk_asset_ids(&note.blocks, &mut |id| {
            referenced_assets.insert(id);
        });
    }
    let unreferenced_asset_ids: Vec<AssetId> = doc
        .assets
        .iter()
        .map(|asset| asset.id)
        .filter(|id| !referenced_assets.contains(id))
        .collect();

    if !note_markdown.is_empty() || !note_asset_ids.is_empty() || !unreferenced_asset_ids.is_empty()
    {
        let part = trailing_unowned_part(&mut parts, doc.blocks.len());
        if !note_markdown.is_empty() {
            if !part.markdown.is_empty() {
                part.markdown.push_str("\n\n");
            }
            part.markdown.push_str(&note_markdown);
        }
        for id in note_asset_ids.into_iter().chain(unreferenced_asset_ids) {
            if !part.asset_ids.contains(&id) {
                part.asset_ids.push(id);
            }
        }
    }

    for part in &mut parts {
        part.markdown = finish_markdown(std::mem::take(&mut part.markdown));
    }
    parts
}

fn render_part(
    doc: &Document,
    rc: &Ctx,
    source_unit_index: Option<usize>,
    start_block: usize,
    end_block: usize,
    referenced_assets: &mut HashSet<AssetId>,
) -> RenderedPart {
    let blocks = &doc.blocks[start_block..end_block];
    let asset_ids = collect_asset_ids(blocks, referenced_assets);
    RenderedPart {
        markdown: render_blocks(blocks, rc),
        source_unit_index,
        start_block,
        end_block,
        asset_ids,
    }
}

fn trailing_unowned_part(parts: &mut Vec<RenderedPart>, block_count: usize) -> &mut RenderedPart {
    let has_trailing_unowned = parts
        .last()
        .is_some_and(|part| part.source_unit_index.is_none() && part.end_block == block_count);
    if !has_trailing_unowned {
        parts.push(RenderedPart {
            markdown: String::new(),
            source_unit_index: None,
            start_block: block_count,
            end_block: block_count,
            asset_ids: Vec::new(),
        });
    }
    parts.last_mut().expect("a trailing part was retained or inserted")
}

fn collect_asset_ids(blocks: &[Block], all_seen: &mut HashSet<AssetId>) -> Vec<AssetId> {
    let mut part_seen = HashSet::new();
    let mut ids = Vec::new();
    walk_asset_ids(blocks, &mut |id| {
        all_seen.insert(id);
        if part_seen.insert(id) {
            ids.push(id);
        }
    });
    ids
}

fn collect_asset_ids_from_notes<'a>(
    notes: impl IntoIterator<Item = &'a Note>,
    all_seen: &mut HashSet<AssetId>,
) -> Vec<AssetId> {
    let mut note_seen = HashSet::new();
    let mut ids = Vec::new();
    for note in notes {
        walk_asset_ids(&note.blocks, &mut |id| {
            all_seen.insert(id);
            if note_seen.insert(id) {
                ids.push(id);
            }
        });
    }
    ids
}

fn walk_asset_ids(blocks: &[Block], found: &mut impl FnMut(AssetId)) {
    fn walk_inlines(inlines: &[Inline], found: &mut impl FnMut(AssetId)) {
        for inline in inlines {
            match inline {
                Inline::Link { content, .. } => walk_inlines(content, found),
                Inline::Image { source: crate::model::ImageSource::Asset(id), .. } => found(*id),
                Inline::Text { .. }
                | Inline::Image { .. }
                | Inline::Anchor(_)
                | Inline::NoteRef(_)
                | Inline::LineBreak => {}
            }
        }
    }

    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph(content) => {
                walk_inlines(content, found)
            }
            Block::List(list) => {
                for item in &list.items {
                    walk_asset_ids(&item.blocks, found);
                }
            }
            Block::Table(table) => {
                for row in &table.grid {
                    for slot in row {
                        if let crate::model::CellSlot::Origin(cell) = slot {
                            walk_asset_ids(&cell.blocks, found);
                        }
                    }
                }
            }
            Block::BlockQuote(blocks) => walk_asset_ids(blocks, found),
            Block::CodeBlock { .. } | Block::Rule => {}
        }
    }
}

fn render_source_unit(unit: &crate::model::SourceUnit, start: bool) -> String {
    let kind = match unit.kind {
        crate::model::SourceUnitKind::Slide => "slide",
        crate::model::SourceUnitKind::Sheet => "sheet",
    };
    let boundary = if start { "start" } else { "end" };
    let status = match unit.status {
        crate::model::SourceUnitStatus::Parsed => "parsed",
        crate::model::SourceUnitStatus::Empty => "empty",
        crate::model::SourceUnitStatus::Skipped => "skipped",
    };
    let mut marker = format!(
        "<!-- anydoc:source-unit-{boundary} kind={kind} ordinal={} status={status} ",
        unit.ordinal
    );
    if start && let Some(name) = &unit.name {
        marker.push_str("name=");
        marker.push_str(&percent_encode(name));
        marker.push(' ');
    }
    if start && let Some(reason) = &unit.reason {
        marker.push_str("reason=");
        marker.push_str(&percent_encode(reason));
        marker.push(' ');
    }
    marker.push_str("-->");
    marker
}

/// Percent-encode source names so untrusted text cannot close the HTML
/// comment or introduce ambiguous whitespace-delimited fields.
fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

/// Number notes in first-reference order; unreferenced notes follow at the
/// end. The first note wins a duplicated id.
fn number_notes(doc: &Document) -> NoteNumbers {
    let mut valid: HashMap<&str, &Note> = HashMap::new();
    for note in &doc.notes {
        if !note.blocks.iter().all(block_is_blank) {
            valid.entry(note.id.as_str()).or_insert(note);
        }
    }
    let mut order: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    collect_note_refs(&doc.blocks, &valid, &mut order, &mut seen);
    for note in &doc.notes {
        if valid.contains_key(note.id.as_str()) && seen.insert(note.id.clone()) {
            order.push(note.id.clone());
        }
    }
    order.into_iter().enumerate().map(|(i, id)| (id, i + 1)).collect()
}

fn block_is_blank(block: &Block) -> bool {
    match block {
        Block::Paragraph(inlines) => inlines_are_empty(inlines),
        _ => false,
    }
}

fn collect_note_refs(
    blocks: &[Block],
    valid: &HashMap<&str, &Note>,
    order: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    fn walk_inlines(
        inlines: &[Inline],
        valid: &HashMap<&str, &Note>,
        order: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        for inline in inlines {
            match inline {
                Inline::NoteRef(id) => {
                    if let Some(note) = valid.get(id.as_str())
                        && seen.insert(id.clone())
                    {
                        order.push(id.clone());
                        collect_note_refs(&note.blocks, valid, order, seen);
                    }
                }
                Inline::Link { content, .. } => walk_inlines(content, valid, order, seen),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Paragraph(i) | Block::Heading { content: i, .. } => {
                walk_inlines(i, valid, order, seen)
            }
            Block::List(list) => {
                for item in &list.items {
                    collect_note_refs(&item.blocks, valid, order, seen);
                }
            }
            Block::Table(t) => {
                for row in &t.grid {
                    for slot in row {
                        if let crate::model::CellSlot::Origin(cell) = slot {
                            collect_note_refs(&cell.blocks, valid, order, seen);
                        }
                    }
                }
            }
            Block::BlockQuote(blocks) => collect_note_refs(blocks, valid, order, seen),
            Block::CodeBlock { .. } | Block::Rule => {}
        }
    }
}

fn render_blocks(blocks: &[Block], rc: &Ctx) -> String {
    let parts: Vec<String> = blocks.iter().filter_map(|b| render_block(b, rc)).collect();
    parts.join("\n\n")
}

fn render_block(block: &Block, rc: &Ctx) -> Option<String> {
    match block {
        Block::Heading { level, content, .. } => {
            let text = render_inlines(content, InlineContext::Heading, rc);
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            let level = (*level).clamp(1, 6) as usize;
            Some(format!("{} {}", "#".repeat(level), text))
        }
        Block::Paragraph(inlines) => {
            let text = render_inlines(inlines, InlineContext::Block, rc);
            let trimmed = trim_paragraph(&text);
            if trimmed.is_empty() { None } else { Some(trimmed) }
        }
        Block::List(list) => render_list(list, rc),
        // Trivial layout tables are scaffolding; render their content directly.
        Block::Table(t) if t.kind == TableKind::Layout && t.is_single_cell() => {
            let crate::model::CellSlot::Origin(cell) = &t.grid[0][0] else { unreachable!() };
            let inner = render_blocks(&cell.blocks, rc);
            if inner.is_empty() { None } else { Some(inner) }
        }
        Block::Table(t) => table::render_table(t, rc),
        Block::BlockQuote(blocks) => {
            let inner = render_blocks(blocks, rc);
            if inner.is_empty() {
                return None;
            }
            let quoted: Vec<String> = inner
                .lines()
                .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
                .collect();
            Some(quoted.join("\n"))
        }
        Block::CodeBlock { lang, text } => {
            let fence = backtick_fence(text, 3);
            let lang = lang.as_deref().unwrap_or("");
            let body = text.trim_end_matches('\n');
            Some(format!("{fence}{lang}\n{body}\n{fence}"))
        }
        Block::Rule => Some("---".to_string()),
    }
}

fn render_list(list: &List, rc: &Ctx) -> Option<String> {
    if list.items.is_empty() {
        return None;
    }
    let mut rendered_items: Vec<String> = Vec::new();
    let mut loose = false;
    for (i, item) in list.items.iter().enumerate() {
        // GFM has decimal ordered lists only, so Roman/alphabetic levels
        // render as bullets carrying the source marker as literal text
        // (`- iv. …`) — the source marker semantics stay visible. Items with
        // an explicit label (composite number text) render it the same way.
        let marker = match (&item.marker_label, list.marker) {
            (Some(label), _) => {
                format!("- {} ", escape_marker_label(label, InlineContext::Block))
            }
            (None, MarkerKind::Bullet) => "- ".to_string(),
            (None, MarkerKind::Decimal) => format!("{}. ", list.start.saturating_add(i as u64)),
            (None, kind) => format!("- {} ", kind.label(list.start.saturating_add(i as u64))),
        };
        let checkbox = match item.checked {
            Some(true) => "[x] ",
            Some(false) => "[ ] ",
            None => "",
        };
        let body = render_blocks(&item.blocks, rc);
        if item.blocks.len() > 1 {
            loose = true;
        }
        let indent = " ".repeat(marker.chars().count());
        let mut lines = body.lines();
        let first = lines.next().unwrap_or("");
        let mut s = format!("{marker}{checkbox}{first}");
        for line in lines {
            s.push('\n');
            if line.is_empty() {
                loose = true;
            } else {
                s.push_str(&indent);
                s.push_str(line);
            }
        }
        rendered_items.push(s);
    }
    let sep = if loose { "\n\n" } else { "\n" };
    Some(rendered_items.join(sep))
}

/// Trim paragraph lines, keeping hard-break backslashes intact.
fn trim_paragraph(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| {
            let t = l.trim_start();
            let t = if ends_with_hard_break(t) { t } else { t.trim_end() };
            if t.trim_end_matches('\\').trim().is_empty() { "" } else { t }
        })
        .collect();
    let start = lines.iter().position(|l| !l.is_empty());
    let end = lines.iter().rposition(|l| !l.is_empty());
    match (start, end) {
        (Some(s), Some(e)) => {
            let mut out = lines[s..=e].join("\n");
            if ends_with_hard_break(&out) {
                out.pop();
                out.truncate(out.trim_end().len());
            }
            out
        }
        _ => String::new(),
    }
}

fn ends_with_hard_break(line: &str) -> bool {
    line.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}
