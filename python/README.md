# zeroset-anydoc

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/zeroset-inc/anydoc/blob/main/LICENSE)

Convert Word, PowerPoint, Excel, OpenDocument, RTF, EPUB, CSV, and PDF files into clean GitHub-Flavored Markdown. Python bindings for the [anydoc](https://github.com/firecrawl/anydoc) Rust crate, built by [Firecrawl](https://firecrawl.dev). Also available as a hosted API through [Firecrawl Parse](https://firecrawl.dev/parse), which adds our OCR models for the scanned pages anydoc can't read on its own.

Every format parses into one shared document model and renders through a single Markdown serializer, so headings, tables, lists, and footnotes come out the same no matter which format goes in. Conversion releases the GIL, so other threads keep running. Type stubs ship with the package.

```bash
pip install zeroset-anydoc
```

The package installs as `zeroset-anydoc` and imports as `anydoc`.

## Supported formats

| Format           | Extensions                                                 |
| ---------------- | ---------------------------------------------------------- |
| Word             | `.doc`, `.docx`, `.docm`                                   |
| PowerPoint       | `.ppt`, `.pps`, `.pot`, `.pptx`, `.pptm`, `.ppsx`, `.ppsm` |
| Excel            | `.xls`, `.xlsx`, `.xlsm`, `.xlsb`                          |
| OpenDocument     | `.odt`, `.ods`, `.odp`                                     |
| Rich Text Format | `.rtf`                                                     |
| EPUB             | `.epub`                                                    |
| CSV              | `.csv`                                                     |
| PDF              | `.pdf`                                                     |

## Usage

```python
import anydoc

# From a file path:
markdown = anydoc.to_markdown("report.docx")

# From bytes, with the format detected from the content:
markdown = anydoc.to_markdown_bytes(data)

# Or name it, which signature-less formats (CSV) need:
markdown = anydoc.to_markdown_bytes(data, "csv")

# Or get Markdown and the document model from one parse. Rendered parts map
# source units and embedded assets back to exact ranges in document.blocks:
document = anydoc.to_document(data)
markdown = document.markdown
for part in document.rendered_parts:
    blocks = document.blocks[part.start_block : part.end_block]
    assets = [document.assets[asset_id] for asset_id in part.asset_ids]

# Skip the Python document graph when only rendered text and provenance are
# needed:
rendered = anydoc.to_rendered_parts(data)
for part in rendered.parts:
    source = (
        rendered.source_units[part.source_unit_index]
        if part.source_unit_index is not None
        else None
    )

# Read bounded XLSX/XLSM image ownership without parsing spreadsheet cells:
manifest = anydoc.extract_spreadsheet_assets(xlsx_data)
for sheet in manifest.source_units:
    images = [manifest.assets[asset_id] for asset_id in sheet.asset_ids]

# Bound application-level payload retention without losing text, asset ids,
# or part/sheet provenance. Omitted assets have data=None plus a reason.
policy = anydoc.AssetRetentionPolicy(
    max_unique_assets=200,
    max_total_bytes=32 * 1024 * 1024,
    max_asset_bytes=32 * 1024 * 1024,
)
document = anydoc.to_document(data, asset_policy=policy)
omitted = [asset for asset in document.assets if asset.data is None]
```

## Errors

A conversion raises only when no meaningful Markdown could come out of the file. The exception type names what went wrong:

```python
try:
    return anydoc.to_markdown(path)
except (anydoc.EncryptedError, anydoc.UnsupportedError) as error:
    # No document comes out of these, so record the file and take the next one.
    unconverted.append((path, type(error).__name__))
    return None
```

| Exception            | Raised when                                                         |
| -------------------- | ------------------------------------------------------------------- |
| `UnsupportedError`   | Unknown format, or one that cannot be converted (an image-only PDF) |
| `MalformedError`     | Structurally unusable: no meaningful content could be extracted     |
| `EncryptedError`     | Encrypted or password-protected                                     |
| `ResourceLimitError` | Crossed a fixed safety limit (decompression, nesting, node count)   |
| `MissingPartError`   | A part required for any meaningful output is absent                 |
| `OSError`            | The file could not be read, from `to_markdown` only                 |

The five conversion failures subclass `anydoc.ConvertError`, so catching that handles all of them at once. `MalformedError.part` and `MissingPartError.part` name the package part at fault, `ResourceLimitError.limit` names the limit crossed, and `str(error)` carries the whole message. A `format` argument naming no supported format raises `ValueError`.

## Format detection

The format is read from the file content, using the marker its specification designates: the PDF header, the RTF open group, OLE stream names, the ZIP package mimetype and content types. CSV has no such marker, so detection returns `None` for it and the extension, or an explicit format, names it instead.

```python
anydoc.format_from_bytes(data)  # 'docx', or None when nothing matches
anydoc.format_from_extension(".pptm")  # 'pptx'
anydoc.format_from_path("report.odt")  # 'odt'
```

## Images and embedded objects

Markdown cannot embed bytes, so an embedded image renders as its alt text while its record stays on `document.assets`, tagged with a media type and source part. By default `asset.data` carries the bytes. An `AssetRetentionPolicy` can omit payloads by unique count, aggregate bytes, or individual bytes; the same asset id remains in rendered parts and source units, while `data`, `byte_len`, and `omission_reason` expose the structured outcome. Images that carry an external URL render as ordinary Markdown images. Standard XLSX/XLSM DrawingML images retain their sheet and bounded cell placement.

Presentation slides and spreadsheet sheets are exposed through `document.source_units`. Each unit carries its 1-based ordinal, optional source name, extraction status, and a half-open range into `document.blocks`; empty and skipped units are retained.

`document.rendered_parts` is the ordered partition for consuming that model. Each part carries Markdown for its block range, the corresponding source-unit index when one exists, and recursively discovered embedded asset ids. Gaps and assets without honest source-unit provenance remain in unowned parts rather than being attributed to the nearest slide or sheet.

Full behavior notes and benchmarks live in the [repository README](https://github.com/zeroset-inc/anydoc#readme).

## License

[MIT](https://github.com/zeroset-inc/anydoc/blob/main/LICENSE)
