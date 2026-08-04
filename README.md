# anydoc

[![Crates.io](https://img.shields.io/crates/v/anydoc.svg)](https://crates.io/crates/anydoc)
[![npm](https://img.shields.io/npm/v/@firecrawl/anydoc.svg)](https://www.npmjs.com/package/@firecrawl/anydoc)
[![PyPI](https://img.shields.io/pypi/v/firecrawl-anydoc.svg)](https://pypi.org/project/firecrawl-anydoc/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Fast Rust library that converts documents (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB, CSV, and PDF) into clean GitHub-Flavored Markdown. Includes bindings for [Node.js](node/README.md) and [Python](python/README.md).

Built by [Firecrawl](https://firecrawl.dev) to turn any office document into LLM-ready Markdown in single-digit milliseconds, with one consistent output no matter which format goes in. It powers [Firecrawl Parse](https://firecrawl.dev/parse), so if you'd rather not run it yourself, the hosted API gives you the same conversion plus our OCR models for the scanned pages anydoc can't read on its own.

## Features

- **One output for every format.** Each format parses into a shared document model and renders through a single Markdown serializer, so escaping, tables, heading anchors, and footnotes behave identically whether the input was a `.doc` from 2003 or a `.pptx` from yesterday.
- **Full document structure.** Headings with anchors, bold/italic/strikethrough, inline code and code blocks, links and internal cross-references, bulleted/numbered/nested/task lists with the source's own numbering, tables with merged cells and header rows, block quotes, footnotes and endnotes, and speaker notes.
- **Embedded assets.** Images and embedded objects render as their alt text in the Markdown, and the raw bytes stay available on the document model, tagged with their media type. Images with an external URL become ordinary Markdown images.
- **Content-based format detection.** The format is read from the bytes themselves (PDF header, RTF open group, OLE stream names, ZIP package mimetype), so mislabeled files still convert correctly.
- **Fast.** Pure Rust, no ML models, no external services. Median conversion time is under 5ms per document.
- **Bindings that stay out of the way.** Node.js conversion runs on the libuv thread pool and never blocks the event loop; Python releases the GIL so other threads keep running. TypeScript types and Python stubs ship with the packages.
- **PDF support built in.** Text-based PDFs convert locally through [pdf-inspector](https://github.com/firecrawl/pdf-inspector), no OCR service required.

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

PDFs take a shortcut: [pdf-inspector](https://github.com/firecrawl/pdf-inspector) emits Markdown directly, so use `to_markdown` / `to_markdown_bytes` for them rather than `to_document`. Scanned and image-only PDFs need OCR, so anydoc returns an unsupported error for them; route those to [Firecrawl Parse](https://docs.firecrawl.dev/api-reference/endpoint/parse), which OCRs them and returns the same Markdown.

## Benchmark

anydoc is measured against six other converters on 100 real-world documents spanning fourteen formats. Scores run from 0 to 100, higher is better; speed is the median time to convert one document.

| tool         | formats   | median ms | docs judged | score  | completeness | structure | formatting | cleanliness |
| ------------ | --------- | --------- | ----------- | ------ | ------------ | --------- | ---------- | ----------- |
| anydoc       | **14/14** | **4.7**   | 94          | **80** | **88**       | **78**    | **77**     | **79**      |
| libreoffice  | 12/14     | 1129.5    | 87          | 40     | 59           | 43        | 43         | 24          |
| unstructured | 8/14      | 572.9     | 58          | 65     | 76           | 62        | 52         | 67          |
| markitdown   | 6/14      | 134.8     | 33          | 65     | 80           | 67        | 61         | 53          |
| pandoc       | 5/14      | 102.1     | 34          | 57     | 75           | 57        | 58         | 39          |
| docling      | 4/14      | 513.6     | 21          | 57     | 63           | 59        | 57         | 52          |
| mammoth      | 1/14      | 52.5      | 8           | 70     | 85           | 68        | 74         | 55          |

Per format, like for like:

| format | anydoc | libreoffice | unstructured | markitdown | pandoc | docling | mammoth |
| ------ | ------ | ----------- | ------------ | ---------- | ------ | ------- | ------- |
| doc    | **88** | 58          | 68           | -          | -      | -       | -       |
| docm   | **82** | 49          | -            | -          | -      | -       | -       |
| docx   | **86** | 53          | 56           | 72         | 68     | 68      | 70      |
| epub   | 74     | -           | 74           | **77**     | 53     | -       | -       |
| odp    | **87** | 22          | -            | -          | -      | -       | -       |
| ods    | **82** | 42          | -            | -          | -      | -       | -       |
| odt    | **80** | 52          | 70           | -          | 61     | -       | -       |
| ppt    | **80** | 25          | -            | -          | -      | -       | -       |
| pptx   | **76** | 22          | -            | 59         | -      | 50      | -       |
| rtf    | **89** | 58          | 48           | -          | 46     | -       | -       |
| xls    | **77** | 40          | 68           | 64         | -      | -       | -       |
| xlsm   | **70** | 30          | -            | -          | -      | -       | -       |
| xlsx   | **70** | 31          | 69           | 55         | -      | 51      | -       |

**How quality was scored:** an LLM judge (Claude Sonnet 5) compares two tools' outputs blind against ground truth: the document's first six pages, rendered to images by LibreOffice. Each output is scored on completeness, structure, formatting, and cleanliness. Every pair is judged twice with the outputs swapped to cancel position bias, for 479 verdicts in total. Each tool's `score` averages its per-format scores over the formats it supports, so a corpus heavy in one format can't skew it. It also means each row averages a different set of formats (mammoth's 70 is docx alone, while anydoc's 80 spans all fourteen), so the per-format table is the fair comparison.

Speed is one warm conversion per document on a Ryzen 9 9950X3D (Windows 11, 64 GB DDR5-6400). anydoc and the Python libraries are timed with process spawn excluded; the CLI tools include it, since that is how they are used. The harness lives in [`bench/`](bench/README.md); the corpus is not redistributable and is not in the repo.

**Best fit:** pipelines that receive a mixed bag of office documents and need one consistent, structured Markdown output. In this comparison, anydoc was the only tool to cover all fourteen formats, scored highest on every judged format except EPUB, and converted documents an order of magnitude faster than the next-fastest tool.

## Quick start

### Node.js

```bash
npm install @firecrawl/anydoc
```

```js
import { toDocument, toMarkdown, toMarkdownBytes } from '@firecrawl/anydoc';

// From a file path:
const markdown = await toMarkdown('report.docx');

// From bytes, with the format detected from the content:
const fromBytes = await toMarkdownBytes(bytes);

// Or name it, which signature-less formats (CSV) need:
const fromCsv = await toMarkdownBytes(bytes, 'csv');

// Or stop at the document model, which also carries embedded assets:
const document = await toDocument(bytes);
```

> Full API reference: [node/README.md](node/README.md)

### Python

```bash
pip install firecrawl-anydoc
```

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

> Full API reference: [python/README.md](python/README.md)

### Rust

```bash
cargo add anydoc
```

```rust
// From a file path:
let markdown = anydoc::to_markdown("report.docx")?;

// From bytes, with the format detected from the content:
let markdown = anydoc::to_markdown_bytes(&bytes, None)?;

// Or name it, which signature-less formats (CSV) need:
let markdown = anydoc::to_markdown_bytes(&bytes, anydoc::Format::Csv)?;

// Or stop at the document model, which also carries embedded assets:
let document = anydoc::to_document(&bytes, None)?;
```

### CLI

A convert CLI ships in [`examples/`](examples/), in all three languages:

```bash
cargo run --release --example convert -- file.docx [-f csv] [-o out.md] [--assets dir]
node examples/convert.mjs file.docx [-f csv] [-o out.md] [--assets dir]
python examples/convert.py file.docx [-f csv] [-o out.md] [--assets dir]
```

## Format detection

The format is read from the file content, using the marker its specification designates: the PDF header, the RTF open group, OLE stream names, the ZIP package mimetype and content types. CSV has no such marker, so the extension or an explicit format names it instead.

```rust
Format::from_bytes(&bytes); // Some(Format::Docx), or None when nothing matches
Format::from_extension("pptm"); // Some(Format::Pptx)
Format::from_path(Path::new("report.odt")); // Some(Format::Odt)
```

The same three functions exist in Node (`formatFromBytes`, ...) and Python (`anydoc.format_from_bytes`, ...).

## Source provenance

`to_document` preserves presentation slides and spreadsheet sheets in `document.source_units` (`sourceUnits` in Node). Each unit carries its 1-based ordinal, optional source name, extraction status, and a half-open range into the top-level block list. Empty and skipped units remain visible instead of shifting later page or sheet numbers. Markdown output preserves the same boundaries in paired HTML comments.

Standard XLSX/XLSM DrawingML images are retained in `document.assets` and placed in their anchored cells when those cells are inside the used range. Image-only, absolute-positioned, and distant-anchor images remain scoped to their sheet without expanding a sparse grid.

## How it works

```
document bytes
  │
  ├─► format detection      → content markers, not the extension
  │
  ├─► format parser          → one per format (doc, docx, ppt, pptx, xls,
  │                            xlsx, odt/ods/odp, rtf, epub, csv)
  │         │
  │         └─► Document     → shared model: blocks, inlines, tables,
  │                            footnotes, assets
  │               │
  │               └─► GFM serializer → Markdown
  │
  └─► PDF → pdf-inspector    → Markdown directly
```

Because every format funnels through the same document model and serializer, output quirks get fixed once. A table-escaping fix for docx is automatically a table-escaping fix for rtf, odt, and everything else.

## Development

```bash
cargo test
cd node && npm install && npm run build && npm test
cd python && pip install maturin && maturin develop && python -m unittest discover -s tests
```

A committed fixture corpus under `tests/fixtures/` is snapshot-tested, `tests/robustness.rs` mutation-tests every fixture, and `fuzz/` carries cargo-fuzz targets per format. The speed and quality benchmark lives in [`bench/`](bench/README.md).

Releases are tagged `v<version>`, which publishes the crate, the npm package, and the PyPI wheels from [`.github/workflows/release.yml`](.github/workflows/release.yml). The version lives in three places, bumped together for a release:

- [`Cargo.toml`](Cargo.toml): the crate
- [`node/package.json`](node/package.json): the npm package
- [`python/Cargo.toml`](python/Cargo.toml): the wheel (`python/pyproject.toml` reads it)

## License

[MIT](LICENSE)
