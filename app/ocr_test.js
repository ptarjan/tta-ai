'use strict';
/* Node-only test: loads the real iPad screenshot (PNG->BMP via sips,
 * since this app has zero deps and can't decode PNG itself under node),
 * runs the full slot-find -> OCR -> fuzzy-match pipeline, and prints one
 * line per slot. See appocr2_notes.txt for the full analysis. */
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const cards = require('./cards.json');
const {
  findRowSlots, ocrRecognizeSlot, ocrSlotRaw, resolveOCRString, readAgeBadge,
} = require('./app.js');

const PNG = '/Users/pt/tta-ai/analysis/app_samples/ipad_screenshot_2360x1640_2026-08-23.png';
const BMP = path.join(require('os').tmpdir(), 'tta_ocr_test_cap.bmp');

// Display strings: cards.json suffixes the age onto `display` ONLY for
// names shared by more than one age copy (Rich Land, Engineering Genius,
// Frugality, Cultural Heritage here) — see cards.json's `ambiguous` flag.
// The age badge on all 13 cards in this capture reads 'A' (confirmed by
// visual inspection of the crop, cross-checked against cards.json: every
// one of the 9 unambiguous names in this row already IS age A), so the
// age-settled display for those 4 is "<Name> (A)".
const EXPECTED = [
  'Stock Pile', 'Pyramids', 'Rich Land (A)', 'Engineering Genius (A)', 'Hanging Gardens',
  'Julius Caesar', 'Frugality (A)', 'Homer', 'Alexander the Great', 'Moses',
  'Hammurabi', 'Cultural Heritage (A)', 'Colossus',
];

function loadBMP(bmpPath) {
  const buf = fs.readFileSync(bmpPath);
  if (buf[0] !== 0x42 || buf[1] !== 0x4d) throw new Error('not a BMP');
  const dataOffset = buf.readUInt32LE(10);
  const width = buf.readInt32LE(18);
  const heightRaw = buf.readInt32LE(22);
  const bpp = buf.readUInt16LE(28);
  const compression = buf.readUInt32LE(30);
  if (compression !== 0) throw new Error('compressed BMP not supported: ' + compression);
  const height = Math.abs(heightRaw);
  const topDown = heightRaw < 0;
  const bytesPerPixel = bpp / 8;
  const rowSize = Math.floor((bpp * width + 31) / 32) * 4;
  const data = new Uint8ClampedArray(width * height * 4);
  for (let row = 0; row < height; row++) {
    const srcY = topDown ? row : (height - 1 - row);
    const rowStart = dataOffset + srcY * rowSize;
    for (let x = 0; x < width; x++) {
      const srcI = rowStart + x * bytesPerPixel;
      const b = buf[srcI], g = buf[srcI + 1], r = buf[srcI + 2];
      const dstI = (row * width + x) * 4;
      data[dstI] = r; data[dstI + 1] = g; data[dstI + 2] = b; data[dstI + 3] = 255;
    }
  }
  return { data, width, height };
}

execSync(`sips -s format bmp "${PNG}" --out "${BMP}"`, { stdio: 'pipe' });
const pixels = loadBMP(BMP);
fs.unlinkSync(BMP);

const found = findRowSlots(pixels);
if (!found || found.slots.length !== 13) {
  console.log('FAIL: findRowSlots did not return 13 slots');
  process.exit(1);
}

let pass = 0, fail = 0;
found.slots.forEach((slot, i) => {
  const raw = ocrSlotRaw(pixels, slot);
  const badgeAge = readAgeBadge(pixels, slot);
  const resolved = resolveOCRString(raw, cards, badgeAge);
  const got = resolved ? resolved.display : 'UNKNOWN';
  const expected = EXPECTED[i];
  const correct = resolved ? resolved.display === expected : false; // UNKNOWN never counts as correct, but is not a wrong-answer failure
  const wrong = resolved && resolved.display !== expected;
  if (correct) pass++;
  if (wrong) fail++;
  const tag = correct ? 'OK' : wrong ? 'WRONG' : 'UNKNOWN';
  console.log(`slot ${i + 1}: ${JSON.stringify(raw)} badge=${badgeAge} -> ${got}  [${tag}, expected ${expected}]`);
});

const unknownCount = 13 - pass - fail;
console.log('');
console.log(`${pass} passed, ${fail} failed, ${unknownCount} unknown (of 13 slots)`);
process.exit(fail ? 1 : 0);
