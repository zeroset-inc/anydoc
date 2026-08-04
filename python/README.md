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

# Or stop at the document model, which also carries embedded assets:
document = anydoc.to_document(data)
```

## Format detection

The format is read from the file content, using the marker its specification designates: the PDF header, the RTF open group, OLE stream names, the ZIP package mimetype and content types. CSV has no such marker, so detection returns `None` for it and the extension, or an explicit format, names it instead.

```python
anydoc.format_from_bytes(data)  # 'docx', or None when nothing matches
anydoc.format_from_extension(".pptm")  # 'pptx'
anydoc.format_from_path("report.odt")  # 'odt'
```

## Images and embedded objects

Markdown cannot embed bytes, so an embedded image renders as its alt text while the bytes stay on `document.assets`, tagged with a media type and the part they came from. Images that carry an external URL render as ordinary Markdown images. Standard XLSX/XLSM DrawingML images retain their sheet and bounded cell placement.

Presentation slides and spreadsheet sheets are exposed through `document.source_units`. Each unit carries its 1-based ordinal, optional source name, extraction status, and a half-open range into `document.blocks`; empty and skipped units are retained.

Full behavior notes and benchmarks live in the [repository README](https://github.com/zeroset-inc/anydoc#readme).

## License

[MIT](https://github.com/zeroset-inc/anydoc/blob/main/LICENSE)
