'use strict';
/* Node-only test: loads the real iPad screenshots (PNG->BMP via sips,
 * since this app has zero deps and can't decode PNG itself under node),
 * runs the full slot-find -> OCR -> fuzzy-match pipeline against BOTH
 * captures this app has, and prints one line per slot plus a per-capture
 * pass/fail/unknown count. See appatlas_notes.txt for the full analysis
 * of what changed when the second capture was added (glyph atlas widened,
 * a two-line-name splitting bug fixed) and why some capture-B slots stay
 * UNKNOWN on purpose rather than being force-matched. */
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const cards = require('./cards.json');
const {
  findRowSlots, ocrSlotRaw, resolveOCRString, readAgeBadge,
} = require('./app.js');

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

// EXPECTED entries use the card's age-settled display string (see
// resolveOCRString / cards.json's `ambiguous` flag) for names shared by
// more than one age copy; null means the slot is a genuine empty row
// position that findRowSlots must mark .empty and OCR must never touch.
const CAPTURES = [
  {
    label: 'capture A (ipad_screenshot_2360x1640_2026-08-23.png)',
    png: '/Users/pt/tta-ai/analysis/app_samples/ipad_screenshot_2360x1640_2026-08-23.png',
    expected: [
      'Stock Pile', 'Pyramids', 'Rich Land (A)', 'Engineering Genius (A)', 'Hanging Gardens',
      'Julius Caesar', 'Frugality (A)', 'Homer', 'Alexander the Great', 'Moses',
      'Hammurabi', 'Cultural Heritage (A)', 'Colossus',
    ],
  },
  {
    label: 'capture B (ipad_screenshot_2360x1640_2026-08-23_b.png)',
    png: '/Users/pt/tta-ai/analysis/app_samples/ipad_screenshot_2360x1640_2026-08-23_b.png',
    expected: [
      'Rich Land (A)', 'Homer', 'Engineering Genius (A)', 'Library of Alexandria', null,
      'Cultural Heritage (A)', 'Frugality (A)', 'Patriotism (A)', 'Colossus', 'Hanging Gardens',
      'Alexander the Great', 'Rich Land (A)', 'Moses',
    ],
  },
];

let anyFail = false;

for (const { label, png, expected } of CAPTURES) {
  console.log(`=== ${label} ===`);
  const BMP = path.join(require('os').tmpdir(), 'tta_ocr_test_' + path.basename(png) + '.bmp');
  execSync(`sips -s format bmp "${png}" --out "${BMP}"`, { stdio: 'pipe' });
  const pixels = loadBMP(BMP);
  fs.unlinkSync(BMP);

  const found = findRowSlots(pixels);
  if (!found || found.slots.length !== 13) {
    console.log('FAIL: findRowSlots did not return 13 slots');
    anyFail = true;
    continue;
  }

  let pass = 0, fail = 0, emptyOk = 0, emptyWrong = 0;
  found.slots.forEach((slot, i) => {
    const expectedDisplay = expected[i];
    if (slot.empty) {
      const ok = expectedDisplay === null;
      if (ok) emptyOk++; else emptyWrong++;
      console.log(`slot ${i + 1}: EMPTY  [${ok ? 'OK' : 'WRONG'}, expected ${expectedDisplay === null ? 'EMPTY' : expectedDisplay}]`);
      return;
    }
    const raw = ocrSlotRaw(pixels, slot);
    const badgeAge = readAgeBadge(pixels, slot);
    const resolved = resolveOCRString(raw, cards, badgeAge);
    const got = resolved ? resolved.display : 'UNKNOWN';
    const correct = resolved ? resolved.display === expectedDisplay : false;
    const wrong = resolved && resolved.display !== expectedDisplay;
    if (correct) pass++;
    if (wrong) fail++;
    const tag = correct ? 'OK' : wrong ? 'WRONG' : 'UNKNOWN';
    console.log(`slot ${i + 1}: ${JSON.stringify(raw)} badge=${badgeAge} -> ${got}  [${tag}, expected ${expectedDisplay}]`);
  });

  const cardSlots = expected.filter((e) => e !== null).length;
  const unknownCount = cardSlots - pass - fail;
  console.log('');
  console.log(`${pass} passed, ${fail} failed, ${unknownCount} unknown (of ${cardSlots} card slots)` + (emptyWrong ? `; ${emptyWrong} empty slot(s) misread` : ''));
  console.log('');
  if (fail > 0 || emptyWrong > 0) anyFail = true;
}

process.exit(anyFail ? 1 : 0);
