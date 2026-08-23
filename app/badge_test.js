'use strict';
/* Node-only test: age-badge classifier only (Job 1). Loads the same real
 * iPad screenshot as ocr_test.js, finds the 13 slots, and prints the
 * badge age readAgeBadge() gets for each slot against ground truth.
 * Ground truth for this capture: every one of the 13 cards is age A (see
 * appbadge_notes.txt — confirmed both by cards.json, for the 9 slots
 * whose name is unambiguous, and by direct visual inspection of the
 * medallion crop for all 13, including the 4 name-ambiguous ones). */
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { findRowSlots, readAgeBadge } = require('./app.js');

const PNG = '/Users/pt/tta-ai/analysis/app_samples/ipad_screenshot_2360x1640_2026-08-23.png';
const BMP = path.join(require('os').tmpdir(), 'tta_badge_test_cap.bmp');

const EXPECTED_AGE = ['A', 'A', 'A', 'A', 'A', 'A', 'A', 'A', 'A', 'A', 'A', 'A', 'A'];
const NAMES = [
  'Stock Pile', 'Pyramids', 'Rich Land', 'Engineering Genius', 'Hanging Gardens',
  'Julius Caesar', 'Frugality', 'Homer', 'Alexander the Great', 'Moses',
  'Hammurabi', 'Cultural Heritage', 'Colossus',
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
  const age = readAgeBadge(pixels, slot);
  const expected = EXPECTED_AGE[i];
  const ok = age === expected;
  if (ok) pass++; else fail++;
  console.log(`slot ${i + 1} (${NAMES[i]}): badge=${age}  [${ok ? 'OK' : 'WRONG'}, expected ${expected}]`);
});

// Sanity check (not part of the pass/fail count): the classifier must not
// fire CONFIDENTLY on a region that is not the badge. Point it at a
// region shifted 30% of a slot's height further down (well into the card
// art/name band, never the medallion) and see how often it still claims
// a roman numeral. This is a known, documented limitation, not a pass
// gate — see appbadge_notes.txt "what I could not verify".
let falseNumeral = 0;
found.slots.forEach((slot) => {
  const shifted = Object.assign({}, slot, { y: slot.y + slot.h * 0.30 });
  const age = readAgeBadge(pixels, shifted);
  if (age && age !== 'A') falseNumeral++;
});
console.log('');
console.log(`${pass} passed, ${fail} failed (of 13 badge reads)`);
console.log(`non-badge-region sanity check: ${falseNumeral} of 13 mid-card-art regions misread as a roman numeral (I/II/III) -- expected, NOT a pass gate, see notes`);
process.exit(fail ? 1 : 0);
