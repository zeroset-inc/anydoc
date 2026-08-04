// Smoke test: the bindings load and every entry point round-trips a fixture.
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

import {
  formatFromBytes,
  formatFromExtension,
  formatFromPath,
  SourceUnitKind,
  SourceUnitStatus,
  toDocument,
  toMarkdown,
  toMarkdownBytes,
} from './index.js'

const fixture = (name) => fileURLToPath(new URL(`../tests/fixtures/${name}`, import.meta.url))

const OUTLINE = fixture('docx/handmade-outline.docx')
const RICH = fixture('docx/handmade-rich.docx')
const CSV = fixture('csv/sheet.csv')
const PRESENTATION = fixture('pptx/pres.pptx')

test('toMarkdown detects the format from the file content', async () => {
  const markdown = await toMarkdown(OUTLINE)
  assert.match(markdown, /^# /m)
})

test('toMarkdownBytes converts in memory', async () => {
  const markdown = await toMarkdownBytes(await readFile(RICH), 'docx')
  assert.match(markdown, /\| Quarter \| Widgets \|/)
})

test('toMarkdownBytes detects the format when none is named', async () => {
  const markdown = await toMarkdownBytes(await readFile(RICH))
  assert.match(markdown, /\| Quarter \| Widgets \|/)
  // CSV carries no signature, so it has to be named.
  await assert.rejects(toMarkdownBytes(await readFile(CSV)), /unrecognized file content/)
  assert.match(await toMarkdownBytes(await readFile(CSV), 'csv'), /\| --- \|/)
})

test('toDocument exposes the document model', async () => {
  const document = await toDocument(await readFile(OUTLINE), 'docx')
  const heading = document.blocks.find((block) => block.kind === 'heading')
  assert.ok(heading.level >= 1 && heading.level <= 6)
  assert.equal(typeof heading.content[0].text, 'string')
  assert.equal(heading.content[0].kind, 'text')
  assert.equal(typeof heading.content[0].style.bold, 'boolean')
})

test('toDocument carries embedded assets as buffers', async () => {
  const document = await toDocument(await readFile(RICH), 'docx')
  const image = document.assets.find((asset) => asset.mediaType === 'image/png')
  assert.ok(Buffer.isBuffer(image.data))
  assert.ok(image.data.length > 0)
  assert.equal(image.id, document.assets.indexOf(image))
})

test('toDocument exposes source units', async () => {
  const document = await toDocument(await readFile(PRESENTATION), 'pptx')
  assert.equal(document.sourceUnits.length, 2)
  const first = document.sourceUnits[0]
  assert.equal(first.kind, SourceUnitKind.slide)
  assert.equal(first.ordinal, 1)
  assert.equal(first.status, SourceUnitStatus.parsed)
  assert.equal(first.startBlock, 0)
  assert.ok(first.endBlock > first.startBlock)
})

test('format detection reads content, extension, and path', async () => {
  assert.equal(formatFromBytes(await readFile(RICH)), 'docx')
  // CSV carries no signature: only the extension names it.
  assert.equal(formatFromBytes(await readFile(CSV)), null)
  assert.equal(formatFromExtension('.pptm'), 'pptx')
  assert.equal(formatFromExtension('xls'), 'xlsx')
  assert.equal(formatFromPath('/tmp/report.odt'), 'odt')
  assert.equal(formatFromPath('/tmp/report.unknown'), null)
})

test('conversion errors reject with the crate error message', async () => {
  await assert.rejects(toMarkdownBytes(Buffer.from('not a document'), 'docx'), /malformed|unsupported/)
})
