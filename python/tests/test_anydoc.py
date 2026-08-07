"""Smoke test: the bindings load and every entry point round-trips a fixture."""

import ast
import io
import unittest
import zipfile
from pathlib import Path

import anydoc

FIXTURES = Path(__file__).resolve().parents[2] / "tests" / "fixtures"
OUTLINE = FIXTURES / "docx" / "handmade-outline.docx"
RICH = FIXTURES / "docx" / "handmade-rich.docx"
CSV = FIXTURES / "csv" / "sheet.csv"
PRESENTATION = FIXTURES / "pptx" / "pres.pptx"
ENCRYPTED = FIXTURES / "malformed" / "encrypted--errors.odt"
ZIPBOMB = FIXTURES / "abuse" / "zipbomb--errors.docx"
XLS = FIXTURES / "xls" / "sheet.xls"


def _xlsx_with_owned_image() -> bytes:
    package = io.BytesIO()
    with zipfile.ZipFile(package, "w") as archive:
        parts = {
            "[Content_Types].xml": """<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>""",
            "_rels/.rels": """<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>""",
            "xl/workbook.xml": """<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Empty" sheetId="1" r:id="rId1"/><sheet name="Pictures" sheetId="2" r:id="rId2"/></sheets></workbook>""",
            "xl/_rels/workbook.xml.rels": """<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>""",
            "xl/worksheets/sheet1.xml": """<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>""",
            "xl/worksheets/sheet2.xml": """<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rIdDrawing"/></worksheet>""",
            "xl/worksheets/_rels/sheet2.xml.rels": """<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDrawing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>""",
            "xl/drawings/drawing1.xml": """<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="1" cy="1"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="1" name="Owned"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rIdImage"/></xdr:blipFill></xdr:pic></xdr:absoluteAnchor><xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="1" cy="1"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="Missing"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rIdMissing"/></xdr:blipFill></xdr:pic></xdr:absoluteAnchor></xdr:wsDr>""",
            "xl/drawings/_rels/drawing1.xml.rels": """<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/owned.png"/></Relationships>""",
        }
        for name, body in parts.items():
            archive.writestr(name, body)
        archive.writestr("xl/media/owned.png", b"owned-image")
        archive.writestr("xl/media/unreferenced.png", b"not-content")
    return package.getvalue()


class AnydocTest(unittest.TestCase):
    def test_to_markdown_detects_the_format_from_the_file_content(self):
        markdown = anydoc.to_markdown(OUTLINE)
        self.assertRegex(markdown, r"(?m)^# ")

    def test_to_markdown_bytes_converts_in_memory(self):
        markdown = anydoc.to_markdown_bytes(RICH.read_bytes(), "docx")
        self.assertIn("| Quarter | Widgets |", markdown)

    def test_to_markdown_bytes_detects_the_format_when_none_is_named(self):
        markdown = anydoc.to_markdown_bytes(RICH.read_bytes())
        self.assertIn("| Quarter | Widgets |", markdown)
        # CSV carries no signature, so it has to be named.
        with self.assertRaisesRegex(anydoc.ConvertError, "unrecognized file content"):
            anydoc.to_markdown_bytes(CSV.read_bytes())
        self.assertIn("| --- |", anydoc.to_markdown_bytes(CSV.read_bytes(), "csv"))

    def test_to_document_exposes_the_document_model(self):
        document = anydoc.to_document(OUTLINE.read_bytes(), "docx")
        self.assertEqual(document.markdown, anydoc.to_markdown_bytes(OUTLINE.read_bytes(), "docx"))
        heading = next(block for block in document.blocks if block.kind == "heading")
        self.assertTrue(1 <= heading.level <= 6)
        self.assertIsInstance(heading.content[0].text, str)
        self.assertEqual(heading.content[0].kind, "text")
        self.assertIsInstance(heading.content[0].style.bold, bool)

    def test_to_document_carries_embedded_assets_as_bytes(self):
        document = anydoc.to_document(RICH.read_bytes(), "docx")
        image = next(asset for asset in document.assets if asset.media_type == "image/png")
        self.assertIsInstance(image.data, bytes)
        self.assertGreater(len(image.data), 0)
        self.assertEqual(image.byte_len, len(image.data))
        self.assertIsNone(image.omission_reason)
        self.assertEqual(image.id, document.assets.index(image))
        self.assertEqual(
            {asset_id for part in document.rendered_parts for asset_id in part.asset_ids},
            {asset.id for asset in document.assets},
        )

    def test_asset_policy_preserves_text_and_ownership_with_structured_omissions(self):
        data = RICH.read_bytes()
        full = anydoc.to_document(data, "docx")
        limited = anydoc.to_document(
            data,
            "docx",
            asset_policy=anydoc.AssetRetentionPolicy(max_unique_assets=0),
        )

        self.assertEqual(limited.markdown, full.markdown)
        self.assertEqual(len(limited.assets), len(full.assets))
        omitted = next(asset for asset in limited.assets if asset.media_type == "image/png")
        self.assertIsNone(omitted.data)
        self.assertGreater(omitted.byte_len, 0)
        self.assertEqual(omitted.omission_reason, "max_unique_assets")
        self.assertIn(
            omitted.id,
            {asset_id for part in limited.rendered_parts for asset_id in part.asset_ids},
        )

    def test_asset_policy_distinguishes_individual_and_aggregate_budgets(self):
        data = RICH.read_bytes()
        for policy, reason in (
            (anydoc.AssetRetentionPolicy(max_asset_bytes=0), "max_asset_bytes"),
            (anydoc.AssetRetentionPolicy(max_total_bytes=0), "max_total_bytes"),
        ):
            with self.subTest(reason=reason):
                document = anydoc.to_document(data, "docx", asset_policy=policy)
                self.assertTrue(document.assets)
                self.assertEqual(
                    {asset.omission_reason for asset in document.assets},
                    {reason},
                )

    def test_asset_policy_rejects_negative_budgets(self):
        with self.assertRaises(OverflowError):
            anydoc.AssetRetentionPolicy(max_total_bytes=-1)

    def test_to_document_exposes_source_units(self):
        document = anydoc.to_document(PRESENTATION.read_bytes(), "pptx")
        self.assertEqual(len(document.source_units), 2)
        first = document.source_units[0]
        self.assertEqual(first.kind, "slide")
        self.assertEqual(first.ordinal, 1)
        self.assertIsNone(first.name)
        self.assertEqual(first.status, "parsed")
        self.assertIsNone(first.reason)
        self.assertEqual(first.start_block, 0)
        self.assertGreater(first.end_block, first.start_block)
        self.assertEqual(len(document.rendered_parts), len(document.source_units))
        first_part = document.rendered_parts[0]
        self.assertEqual(first_part.source_unit_index, 0)
        self.assertEqual(first_part.start_block, first.start_block)
        self.assertEqual(first_part.end_block, first.end_block)
        self.assertTrue(first_part.markdown)

    def test_to_rendered_parts_matches_document_without_exporting_its_graph(self):
        data = PRESENTATION.read_bytes()
        compact = anydoc.to_rendered_parts(data, "pptx")
        document = anydoc.to_document(data, "pptx")

        self.assertEqual(
            [
                (
                    part.markdown,
                    part.source_unit_index,
                    part.start_block,
                    part.end_block,
                    part.asset_ids,
                )
                for part in compact.parts
            ],
            [
                (
                    part.markdown,
                    part.source_unit_index,
                    part.start_block,
                    part.end_block,
                    part.asset_ids,
                )
                for part in document.rendered_parts
            ],
        )
        self.assertEqual(
            [
                (unit.kind, unit.ordinal, unit.name, unit.status, unit.reason)
                for unit in compact.source_units
            ],
            [
                (unit.kind, unit.ordinal, unit.name, unit.status, unit.reason)
                for unit in document.source_units
            ],
        )
        for absent in ("markdown", "blocks", "notes", "assets", "rendered_parts"):
            self.assertFalse(hasattr(compact, absent), absent)

    def test_to_rendered_parts_preserves_unowned_note_and_asset_parts(self):
        data = RICH.read_bytes()
        compact = anydoc.to_rendered_parts(data, "docx")
        document = anydoc.to_document(data, "docx")

        self.assertEqual(
            [
                (part.markdown, part.source_unit_index, part.asset_ids)
                for part in compact.parts
            ],
            [
                (part.markdown, part.source_unit_index, part.asset_ids)
                for part in document.rendered_parts
            ],
        )
        self.assertTrue(any(part.source_unit_index is None for part in compact.parts))

    def test_spreadsheet_asset_manifest_is_ordered_bounded_and_explicit(self):
        manifest = anydoc.extract_spreadsheet_assets(_xlsx_with_owned_image())

        self.assertEqual(manifest.availability, "available")
        self.assertIsNone(manifest.reason)
        self.assertEqual(
            [(unit.ordinal, unit.name) for unit in manifest.source_units],
            [(1, "Empty"), (2, "Pictures")],
        )
        self.assertEqual(manifest.source_units[0].status, "complete")
        self.assertEqual(manifest.source_units[0].asset_ids, [])
        self.assertEqual(manifest.source_units[1].status, "degraded")
        self.assertEqual(
            manifest.source_units[1].reason,
            "worksheet_drawing_unreadable",
        )
        self.assertEqual(manifest.source_units[1].asset_ids, [0])
        self.assertEqual(len(manifest.assets), 1)
        self.assertEqual(manifest.assets[0].origin_part, "xl/media/owned.png")
        self.assertEqual(manifest.assets[0].data, b"owned-image")

        limited = anydoc.extract_spreadsheet_assets(
            _xlsx_with_owned_image(),
            asset_policy=anydoc.AssetRetentionPolicy(max_unique_assets=0),
        )
        self.assertEqual(limited.source_units[1].asset_ids, [0])
        self.assertIsNone(limited.assets[0].data)
        self.assertEqual(limited.assets[0].byte_len, len(b"owned-image"))
        self.assertEqual(limited.assets[0].omission_reason, "max_unique_assets")

    def test_binary_spreadsheet_assets_are_explicitly_unsupported(self):
        manifest = anydoc.extract_spreadsheet_assets(XLS.read_bytes())

        self.assertEqual(manifest.availability, "unsupported")
        self.assertEqual(
            manifest.reason,
            "binary_spreadsheet_assets_unsupported",
        )
        self.assertEqual(manifest.source_units, [])
        self.assertEqual(manifest.assets, [])

    def test_format_detection_reads_content_extension_and_path(self):
        self.assertEqual(anydoc.format_from_bytes(RICH.read_bytes()), "docx")
        # CSV carries no signature: only the extension names it.
        self.assertIsNone(anydoc.format_from_bytes(CSV.read_bytes()))
        self.assertEqual(anydoc.format_from_extension(".pptm"), "pptx")
        self.assertEqual(anydoc.format_from_extension("xls"), "xlsx")
        self.assertEqual(anydoc.format_from_path("report.odt"), "odt")
        self.assertIsNone(anydoc.format_from_path("report.unknown"))

    def test_conversion_errors_raise_the_subclass_that_names_the_failure(self):
        with self.assertRaises(anydoc.MalformedError) as caught:
            anydoc.to_markdown_bytes(b"not a document", "docx")
        # The base class still catches every one of them.
        self.assertIsInstance(caught.exception, anydoc.ConvertError)
        # Nothing about these bytes is a package part.
        self.assertIsNone(caught.exception.part)

        with self.assertRaises(anydoc.UnsupportedError):
            anydoc.to_markdown_bytes(CSV.read_bytes())

        with self.assertRaises(anydoc.EncryptedError):
            anydoc.to_markdown_bytes(ENCRYPTED.read_bytes(), "odt")

        with self.assertRaises(anydoc.ResourceLimitError) as caught:
            anydoc.to_markdown_bytes(ZIPBOMB.read_bytes(), "docx")
        self.assertEqual(caught.exception.limit, "max_entry_bytes")

        # A readable package carrying none of the parts a docx is made of.
        package = io.BytesIO()
        with zipfile.ZipFile(package, "w") as archive:
            archive.writestr("[Content_Types].xml", "<Types/>")
        with self.assertRaises(anydoc.MissingPartError) as caught:
            anydoc.to_markdown_bytes(package.getvalue(), "docx")
        self.assertEqual(caught.exception.part, "word/document.xml")

    def test_unreadable_files_and_bad_arguments_raise_the_python_exception(self):
        with self.assertRaises(FileNotFoundError):
            anydoc.to_markdown("no-such-file.docx")
        with self.assertRaisesRegex(ValueError, "unknown format"):
            anydoc.to_markdown_bytes(b"", "wat")

    def test_the_stubs_cover_the_module(self):
        stub = Path(anydoc.__file__).with_name("_anydoc.pyi")
        stubbed = {
            node.name
            for node in ast.parse(stub.read_text()).body
            if isinstance(node, (ast.FunctionDef, ast.ClassDef))
        }
        exported = {name for name in dir(anydoc._anydoc) if not name.startswith("_")}
        self.assertEqual(stubbed, exported)
        # __init__.py re-exports the whole module, plus the Format alias.
        self.assertEqual(set(anydoc.__all__), exported | {"Format"})


if __name__ == "__main__":
    unittest.main()
