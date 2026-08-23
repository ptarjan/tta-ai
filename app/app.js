'use strict';
/* TTA Advisor — front end only. See ttaapp_notes.txt for status. */

/* ---------------------------------------------------------------------
 * Engine: one seam. Real wasm today would be a one-line change to
 * point WASM_URL somewhere real; everything else (init/advise contract)
 * is already the real shape. If /tta.wasm fails to load for ANY reason
 * we fall back to a stub and the caller must show the FAKE banner.
 * ------------------------------------------------------------------- */
const WASM_URL = '/tta.wasm';

const Engine = {
  ready: false,
  usingStub: true,
  instance: null,

  async init() {
    try {
      if (typeof WebAssembly === 'undefined') throw new Error('no WebAssembly');
      const resp = await fetch(WASM_URL);
      if (!resp.ok) throw new Error('tta.wasm not found (' + resp.status + ')');
      // instantiateStreaming rejects anything not served as application/wasm,
      // which most trivial static servers get wrong. Compiling the buffer is
      // marginally slower and works regardless of the Content-Type.
      const { instance } = await WebAssembly.instantiate(await resp.arrayBuffer(), {});
      if (!instance.exports.alloc || !instance.exports.advise || !instance.exports.memory) {
        throw new Error('tta.wasm missing expected exports');
      }
      this.instance = instance;
      this.usingStub = false;
    } catch (e) {
      console.warn('[engine] falling back to stub advisor:', e.message);
      this.usingStub = true;
    }
    this.ready = true;
    return !this.usingStub;
  },

  async advise(request) {
    if (!this.ready) await this.init();
    return this.usingStub ? this._stub(request) : this._wasm(request);
  },

  _wasm(request) {
    const { alloc, dealloc, advise, memory } = this.instance.exports;
    const bytes = new TextEncoder().encode(JSON.stringify(request));
    const ptr = alloc(bytes.length);
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    const outPtr = advise(ptr, bytes.length);
    const view = new DataView(memory.buffer);
    const len = view.getUint32(outPtr, true);
    const outBytes = new Uint8Array(memory.buffer, outPtr + 4, len);
    const text = new TextDecoder().decode(outBytes);
    if (dealloc) dealloc(ptr, bytes.length);
    return JSON.parse(text);
  },

  /* Deterministic fake advisor: ranks the row-eligible cards currently in
   * the reported row by a cheap made-up score so the UI is fully
   * testable before the real engine exists. NEVER treat this as real
   * advice — the caller is responsible for the FAKE banner. */
  _stub(request) {
    const rowLine = (request.lines || []).find((l) => l.startsWith('row '));
    const names = rowLine ? rowLine.slice(4).split(',').map((s) => s.trim()) : [];
    const moves = names
      .map((name, i) => ({ name, slot: i }))
      .filter((x) => x.name && x.name !== '.')
      .map((x) => {
        const seed = hashStr(x.name + '|' + request.seed);
        const score = Math.round(((seed % 200) - 100) / 10 - x.slot * 0.3) / 10 * 10;
        return {
          text: `TAKE '${x.name}' from row slot ${x.slot + 1}  [${x.slot + 1} civil action(s)]`,
          score,
          detail: 'stub heuristic: random-ish, penalizes higher slot cost',
        };
      })
      .sort((a, b) => b.score - a.score)
      // The real engine returns a SEQUENCE of moves it played this turn, so
      // the stub emits a plausible short sequence too -- a stub that returned
      // a ranked menu would train the UI on a shape the engine never sends.
      .slice(0, 2);
    moves.push({ text: 'END YOUR TURN (production, then pass the board on)', score: 0, detail: 'stub always ends the turn here' });
    const prevTurn = (request.state && request.state.turn) || 0;
    return {
      ok: true,
      moves,
      state: { turn: prevTurn + 1, stub: true },
      position: { round: '?', age: 'STUB', civil_actions: '?', military_actions: '?', food: '?', resources: '?', science: '?' },
    };
  },
};

function hashStr(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return Math.abs(h);
}

/* ---------------------------------------------------------------------
 * Slot finder — vendored from findRowSlots.js (calibrated separately
 * against a real iPad capture; see /private/tmp/slotcal/notes.txt at the
 * time this was vendored). Copied in whole, comments intact, rather than
 * added as a second <script> tag: the app loads exactly one script and
 * this keeps it that way. Zero deps, pure ES2020, no DOM.
 *
 * Rule this relies on: the Through the Ages card row is drawn as THREE
 * bracketed groups of cards (take-cost banding) with a visible background
 * gap between groups, and a thin painted border between individual cards
 * within the same group. That grouping is fixed by the game rules (same at
 * every player count), not by resolution, so it is used as the structural
 * landmark: exactly two of the twelve boundaries between the thirteen card
 * slots are "wide" (inter-group background gaps) and the other ten are
 * "narrow" (inter-card borders).
 *
 * Detection is texture-based and resolution-independent (all outputs are
 * fractions of image width/height, 0..1). The row band is found by
 * smoothed row-texture + Otsu. Card dividers are found by a VALLEY
 * threshold on vertical column texture — Otsu was tried there FIRST and
 * FAILED (it landed inside the high-texture card-interior mass instead of
 * the tight low-texture border/gap cluster, because border/gap columns are
 * a small minority sitting near zero while card-interior columns have huge
 * internal spread; Otsu's between-class-variance objective preferred
 * splitting the spread-out majority over isolating the tight minority).
 * The two widest boundary runs are classified as the inter-group gaps.
 * Returns null on anything short of a clean 13-slot / 3-group result —
 * a wrong guess would silently corrupt game state, a null just asks a
 * human to type the row instead.
 * ------------------------------------------------------------------- */
function findRowSlots(imageData) {
  const { data, width, height } = imageData || {};
  if (!data || !width || !height || width < 20 || height < 20) return null;

  function px(x, y) {
    const i = (y * width + x) * 4;
    return [data[i], data[i + 1], data[i + 2]];
  }

  function otsuThreshold(values) {
    let min = Infinity, max = -Infinity;
    for (let i = 0; i < values.length; i++) {
      const v = values[i];
      if (v < min) min = v;
      if (v > max) max = v;
    }
    if (max <= min) return min;
    const bins = 256;
    const hist = new Array(bins).fill(0);
    const scale = bins / (max - min);
    for (let i = 0; i < values.length; i++) {
      let b = Math.floor((values[i] - min) * scale);
      if (b >= bins) b = bins - 1;
      if (b < 0) b = 0;
      hist[b]++;
    }
    const n = values.length;
    let sumAll = 0;
    for (let i = 0; i < bins; i++) sumAll += i * hist[i];
    let sumB = 0, wB = 0, best = -1, bestBin = 0;
    for (let i = 0; i < bins; i++) {
      wB += hist[i];
      if (wB === 0) continue;
      const wF = n - wB;
      if (wF === 0) break;
      sumB += i * hist[i];
      const mB = sumB / wB;
      const mF = (sumAll - sumB) / wF;
      const varBetween = wB * wF * (mB - mF) * (mB - mF);
      if (varBetween > best) { best = varBetween; bestBin = i; }
    }
    return min + bestBin / scale;
  }

  // Otsu balances two roughly-comparable-mass classes; it is used for the
  // row band, where "row" vs "background" rows are both a sizeable share of
  // the image height. It is a poor fit for the column pass, where border/gap
  // columns are a small minority (~5-10%) sitting in a tight cluster near
  // zero: Otsu's optimum there tends to land inside the high-texture card
  // mass rather than at the true valley. valleyThreshold instead looks for
  // the single biggest jump between consecutive values in the lower half of
  // the sorted data - that jump is the gap between "uniform border/gap
  // column" values and "textured card interior" values.
  function valleyThreshold(values) {
    const sorted = Float64Array.from(values).sort();
    const n = sorted.length;
    const limit = Math.max(2, Math.floor(n * 0.5));
    let bestGap = -1, bestIdx = 1;
    for (let i = 1; i < limit; i++) {
      const gap = sorted[i] - sorted[i - 1];
      if (gap > bestGap) { bestGap = gap; bestIdx = i; }
    }
    return (sorted[bestIdx] + sorted[bestIdx - 1]) / 2;
  }

  function longestRun(values, thresh, n) {
    let bestStart = -1, bestEnd = -1, bestLen = 0, curStart = -1;
    for (let i = 0; i < n; i++) {
      if (values[i] >= thresh) {
        if (curStart === -1) curStart = i;
      } else if (curStart !== -1) {
        const len = i - curStart;
        if (len > bestLen) { bestLen = len; bestStart = curStart; bestEnd = i - 1; }
        curStart = -1;
      }
    }
    if (curStart !== -1) {
      const len = n - curStart;
      if (len > bestLen) { bestLen = len; bestStart = curStart; bestEnd = n - 1; }
    }
    return bestStart === -1 ? null : { start: bestStart, end: bestEnd };
  }

  function buildRuns(values, thresh, n) {
    const runs = [];
    let curLow = values[0] < thresh;
    let start = 0;
    for (let i = 1; i < n; i++) {
      const low = values[i] < thresh;
      if (low !== curLow) {
        runs.push({ low: curLow, start, end: i - 1 });
        curLow = low;
        start = i;
      }
    }
    runs.push({ low: curLow, start, end: n - 1 });
    return runs;
  }

  function mergeAdjacentLow(runs) {
    const out = [];
    for (const r of runs) {
      const last = out[out.length - 1];
      if (last && last.low && r.low) {
        last.end = r.end;
      } else {
        out.push({ low: r.low, start: r.start, end: r.end });
      }
    }
    return out;
  }

  // box-smooth a signal so a single narrow dip/spike (art detail, a stray
  // icon) doesn't fool the threshold search; window scales with the array
  // length so it stays proportionate at any capture resolution
  function boxSmooth(values, window) {
    const n = values.length;
    const half = Math.floor(window / 2);
    // prefix sums so each window average is O(1) regardless of window size
    const prefix = new Float64Array(n + 1);
    for (let i = 0; i < n; i++) prefix[i + 1] = prefix[i] + values[i];
    const out = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      const lo = Math.max(0, i - half);
      const hi = Math.min(n - 1, i + half);
      out[i] = (prefix[hi + 1] - prefix[lo]) / (hi - lo + 1);
    }
    return out;
  }

  // ---- 1. locate the row band (top/bottom) via row texture ----
  const xStride = Math.max(1, Math.floor(width / 600));
  const rowTexRaw = new Float64Array(height);
  for (let y = 0; y < height; y++) {
    let prev = null, total = 0, n = 0;
    for (let x = 0; x < width; x += xStride) {
      const p = px(x, y);
      if (prev) {
        total += Math.abs(p[0] - prev[0]) + Math.abs(p[1] - prev[1]) + Math.abs(p[2] - prev[2]);
        n++;
      }
      prev = p;
    }
    rowTexRaw[y] = n ? total / n : 0;
  }
  const rowSmoothWindow = Math.max(3, Math.round(height / 60));
  const rowTex = boxSmooth(rowTexRaw, rowSmoothWindow);
  const rowThresh = otsuThreshold(rowTex);
  const band = longestRun(rowTex, rowThresh, height);
  if (!band) return null;
  const yTop = band.start, yBot = band.end;
  const bandH = yBot - yTop + 1;
  if (bandH < height * 0.03) return null; // too thin to be the card row

  // ---- 2. column vertical-texture profile within the band ----
  const yStride = Math.max(1, Math.floor(bandH / 100));
  const colTex = new Float64Array(width);
  for (let x = 0; x < width; x++) {
    let prev = null, total = 0, n = 0;
    for (let y = yTop; y <= yBot; y += yStride) {
      const p = px(x, y);
      if (prev) {
        total += Math.abs(p[0] - prev[0]) + Math.abs(p[1] - prev[1]) + Math.abs(p[2] - prev[2]);
        n++;
      }
      prev = p;
    }
    colTex[x] = n ? total / n : 0;
  }
  const colThresh = valleyThreshold(colTex);

  // ---- 3. build slot (high) / border-or-gap (low) runs, drop outer margins ----
  let runs = buildRuns(colTex, colThresh, width);
  if (runs.length && runs[0].low) runs.shift();
  if (runs.length && runs[runs.length - 1].low) runs.pop();
  if (!runs.length) return null;

  // drop spuriously narrow "high" runs (noise inside a gap/border) and
  // re-merge the low runs that were separated only by that noise
  const highWidths = runs.filter(r => !r.low).map(r => r.end - r.start + 1);
  if (!highWidths.length) return null;
  highWidths.sort((a, b) => a - b);
  const medianHighWidth = highWidths[Math.floor(highWidths.length / 2)];
  runs = runs.filter(r => r.low || (r.end - r.start + 1) >= medianHighWidth * 0.3);
  runs = mergeAdjacentLow(runs);
  if (runs.length && runs[0].low) runs.shift();
  if (runs.length && runs[runs.length - 1].low) runs.pop();

  const slotRuns = runs.filter(r => !r.low);
  const gapRuns = runs.filter(r => r.low);
  if (slotRuns.length !== 13) return null;
  if (gapRuns.length !== 12) return null;

  // ---- 4. classify the 12 boundaries into 2 group gaps + 10 card borders ----
  const gapWidths = gapRuns.map(r => r.end - r.start + 1);
  const sortedWidths = [...gapWidths].sort((a, b) => b - a);
  const groupGapThresh = (sortedWidths[1] + sortedWidths[2]) / 2;

  const groups = [];
  let cur = 1;
  for (let i = 0; i < gapRuns.length; i++) {
    if (gapWidths[i] >= groupGapThresh) {
      groups.push(cur);
      cur = 1;
    } else {
      cur++;
    }
  }
  groups.push(cur);

  if (groups.length !== 3) return null;
  if (groups.reduce((a, b) => a + b, 0) !== 13) return null;

  const slots = slotRuns.map(r => ({
    x: r.start / width,
    y: yTop / height,
    w: (r.end - r.start + 1) / width,
    h: bandH / height,
  }));

  return { slots, groups };
}

/* ---------------------------------------------------------------------
 * Slot hashing — exact-match, not perceptual/fuzzy. A slot rect (fractions
 * of image width/height, from findRowSlots) is resampled by AREA averaging
 * — never absolute pixel offsets — into a fixed GRID x GRID greyscale grid
 * in slot-relative coordinates, so the same card in the same slot hashes
 * identically regardless of the capture's pixel resolution. Each cell is
 * then quantized to one of 32 grey buckets to absorb the residual
 * floating-point noise that different source resolutions introduce into
 * the area-average — this is still exact-match (post-quantization
 * equality), not a distance-threshold fuzzy hash: two grids that land in
 * different buckets anywhere are a different hash, full stop, and a slot
 * that hashes to nothing in the table comes back unknown rather than
 * guessed.
 *
 * Grid size: 12x12. At the reference capture (2360x1640 iPad screenshot)
 * a slot is ~168x200px — 12x12 keeps each cell a coarse patch (~14x17px)
 * so minor resampling/anti-aliasing differences between capture
 * resolutions average out inside a cell rather than landing on a cell
 * boundary, while still being far more than enough resolution to tell
 * ~200 distinct card arts apart (144 cells x 32 grey levels is a huge
 * space relative to the card pool). 8x8 was considered but risked
 * aliasing thin card-art details that help distinguish similarly-colored
 * cards; anything much finer (24x24+) bought no real discriminating power
 * for a card-art-sized source image and made the exact-match hash more
 * sensitive to capture-resolution resampling noise, working against the
 * whole point of area-sampling in the first place.
 * ------------------------------------------------------------------- */
const HASH_GRID = 12;
const HASH_BUCKETS = 32; // 256 grey levels / 8 per bucket

function overlap1d(a0, a1, b0, b1) {
  return Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));
}

// Area-average the greyscale value of imageData within [sx0,sx1)x[sy0,sy1),
// weighting each covered source pixel by how much of it falls inside the
// box (true box-filter resampling), not by nearest/rounded pixel lookup —
// that's what makes the result the same whether the source is native-res
// or a scaled-up/down capture of the same content.
function boxAverageGrey(data, width, height, sx0, sy0, sx1, sy1) {
  const px0 = Math.max(0, Math.floor(sx0)), px1 = Math.min(width, Math.ceil(sx1));
  const py0 = Math.max(0, Math.floor(sy0)), py1 = Math.min(height, Math.ceil(sy1));
  let sum = 0, weightSum = 0;
  for (let py = py0; py < py1; py++) {
    const wy = overlap1d(py, py + 1, sy0, sy1);
    if (wy <= 0) continue;
    for (let px = px0; px < px1; px++) {
      const wx = overlap1d(px, px + 1, sx0, sx1);
      if (wx <= 0) continue;
      const weight = wx * wy;
      const i = (py * width + px) * 4;
      const grey = 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
      sum += grey * weight;
      weightSum += weight;
    }
  }
  return weightSum > 0 ? sum / weightSum : 0;
}

// Resample a slot rect (fractions of image width/height) into a GRID x GRID
// array of quantized grey buckets, entirely in slot-relative coordinates —
// the grid cell boundaries are computed as fractions of the slot's own
// width/height, never as fixed pixel offsets from the image origin.
function slotToGrid(imageData, slot, gridSize) {
  const { data, width, height } = imageData;
  const x0 = slot.x * width, y0 = slot.y * height;
  const w = slot.w * width, h = slot.h * height;
  const cellW = w / gridSize, cellH = h / gridSize;
  const grid = new Uint8Array(gridSize * gridSize);
  for (let gy = 0; gy < gridSize; gy++) {
    const sy0 = y0 + gy * cellH, sy1 = sy0 + cellH;
    for (let gx = 0; gx < gridSize; gx++) {
      const sx0 = x0 + gx * cellW, sx1 = sx0 + cellW;
      const avg = boxAverageGrey(data, width, height, sx0, sy0, sx1, sy1);
      const bucket = Math.max(0, Math.min(HASH_BUCKETS - 1, Math.round((avg / 255) * (HASH_BUCKETS - 1))));
      grid[gy * gridSize + gx] = bucket;
    }
  }
  return grid;
}

// Two independent FNV-1a-style passes over the quantized grid, concatenated
// as hex -> a 16-char (64-bit) string. Short, stable, exact (no distance
// threshold): any single cell landing in a different bucket changes the
// hash. Collisions across a ~200-card pool are astronomically unlikely at
// this width.
function hashGrid(grid) {
  let h1 = 2166136261, h2 = 0x811c9dc5 ^ 0x5bd1e995;
  for (let i = 0; i < grid.length; i++) {
    const v = grid[i];
    h1 ^= v;
    h1 = Math.imul(h1, 16777619);
    h2 ^= (v + i * 131) & 0xff;
    h2 = Math.imul(h2, 2246822519);
    h2 ^= h2 >>> 13;
  }
  return (h1 >>> 0).toString(16).padStart(8, '0') + (h2 >>> 0).toString(16).padStart(8, '0');
}

function hashSlot(imageData, slot) {
  return hashGrid(slotToGrid(imageData, slot, HASH_GRID));
}

/* ---------------------------------------------------------------------
 * Learning table — hash -> card id, in its OWN localStorage key. Kept
 * separate from STORAGE_KEY (the game-state save) on purpose: a corrupt
 * or unparseable hash table must never take the whole app down with it,
 * so a broken read here just degrades to "nothing recognised", never a
 * thrown error at boot.
 * ------------------------------------------------------------------- */
const OCR_HASH_KEY = 'ttaapp_ocr_hashes_v1';

function loadHashTable() {
  try {
    const raw = localStorage.getItem(OCR_HASH_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch (e) { return {}; }
}

function saveHashTable(table) {
  try { localStorage.setItem(OCR_HASH_KEY, JSON.stringify(table)); } catch (e) { /* ignore quota errors */ }
}

function recordHash(hash, cardId) {
  if (!hash || !cardId) return;
  const table = loadHashTable();
  table[hash] = cardId;
  saveHashTable(table);
}

// Holds the imageData + slot rects from the most recent successful scan, so
// that when the user subsequently TYPES a name for a slot the scan left
// unknown (or overrides one it got wrong), that keystroke can teach the
// table — every name typed after a scan improves the next scan. Cleared
// whenever a fresh full-row session starts, so a stale image never teaches
// hashes against a slot layout it doesn't actually match.
let lastScan = null; // { data, width, height, slots } | null

function learnSlotIfScanned(slotIndex, cardId) {
  if (!lastScan || !lastScan.slots || !lastScan.slots[slotIndex]) return;
  const hash = hashSlot(lastScan, lastScan.slots[slotIndex]);
  recordHash(hash, cardId);
}

/* ---------------------------------------------------------------------
 * data: URL -> Blob, with no fetch() involved (fetching a data: URL would
 * work, but this app's policy is no network calls at all, full stop, and
 * keeping data: URLs out of fetch() entirely avoids ever having to reason
 * about whether that counts).
 * ------------------------------------------------------------------- */
function dataUrlToBlob(dataUrl) {
  const match = /^data:([^;,]*)(;base64)?,(.*)$/s.exec(dataUrl);
  if (!match) throw new Error('not a data: URL');
  const mime = match[1] || 'application/octet-stream';
  const isBase64 = !!match[2];
  const payload = match[3];
  const binary = isBase64 ? atob(payload) : decodeURIComponent(payload);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type: mime });
}

// Decode a Blob/File to {data,width,height} RGBA via createImageBitmap +
// canvas. OffscreenCanvas is preferred (works off the main thread and is
// the documented path), but iPad Safari support has varied across versions
// — a silent failure there would be worse than the small cost of a normal
// <canvas> fallback, so this always has a working path in a browser.
async function decodeImageToPixels(blob) {
  const bitmap = await createImageBitmap(blob);
  const w = bitmap.width, h = bitmap.height;
  let ctx;
  if (typeof OffscreenCanvas !== 'undefined') {
    ctx = new OffscreenCanvas(w, h).getContext('2d');
  } else {
    const canvas = document.createElement('canvas');
    canvas.width = w; canvas.height = h;
    ctx = canvas.getContext('2d');
  }
  ctx.drawImage(bitmap, 0, 0);
  if (bitmap.close) bitmap.close();
  const imgData = ctx.getImageData(0, 0, w, h);
  return { data: imgData.data, width: w, height: h };
}

/* ---------------------------------------------------------------------
 * OCR seam. Reads a SCREEN CAPTURE (not a photo — see header notes on why
 * that distinction is load-bearing) of the 13-card row: finds the slot
 * rectangles, hashes each one, and looks each hash up in the learning
 * table. An unknown hash resolves to null for that slot rather than a
 * guess — the caller (renderFullRowStep) pre-fills what it recognises and
 * leaves the rest for the user to type, which is also what teaches the
 * table going forward.
 * ------------------------------------------------------------------- */
async function ocrScanSeam(imageBlobOrDataUrl) {
  const blob = typeof imageBlobOrDataUrl === 'string' ? dataUrlToBlob(imageBlobOrDataUrl) : imageBlobOrDataUrl;
  const pixels = await decodeImageToPixels(blob);
  const found = findRowSlots(pixels);
  if (!found) {
    const err = new Error('Could not find the 13-card row in that image.');
    err.code = 'NO_SLOTS';
    throw err;
  }
  const table = loadHashTable();
  const row = found.slots.map((slot) => {
    const hash = hashSlot(pixels, slot);
    return table[hash] || null;
  });
  lastScan = { data: pixels.data, width: pixels.width, height: pixels.height, slots: found.slots };
  return { row, rivalStr: null, rivalCulture: null, militaryDraws: [] };
}
if (typeof window !== 'undefined') window.ocrScanSeam = ocrScanSeam;

/* ---------------------------------------------------------------------
 * Card database + fuzzy search
 * ------------------------------------------------------------------- */
let CARDS = [];
let CARDS_BY_ID = new Map();
let ROW_POOL = [];
let MIL_POOL = [];

async function loadCards() {
  const resp = await fetch('cards.json');
  CARDS = await resp.json();
  CARDS_BY_ID = new Map(CARDS.map((c) => [c.id, c]));
  ROW_POOL = CARDS.filter((c) => c.rowEligible);
  MIL_POOL = CARDS.filter((c) => !c.rowEligible);
}

function cardDisplay(id) {
  if (!id) return '.';
  const c = CARDS_BY_ID.get(id);
  return c ? c.display : id;
}

// prefix + fuzzy-subsequence match, e.g. "colo" or "clsus" -> Colossus
function matchScore(query, text) {
  query = query.toLowerCase();
  text = text.toLowerCase();
  if (!query) return 0;
  if (text === query) return 1000;
  if (text.startsWith(query)) return 900 - (text.length - query.length);
  const words = text.split(/[^a-z0-9]+/);
  for (const w of words) {
    if (w && w.startsWith(query)) return 800 - (text.length - query.length);
  }
  let ti = 0, gaps = 0;
  for (let qi = 0; qi < query.length; qi++) {
    const ch = query[qi];
    let found = false;
    while (ti < text.length) {
      if (text[ti] === ch) { found = true; ti++; break; }
      ti++; gaps++;
    }
    if (!found) return -1;
  }
  return 400 - gaps - (text.length - query.length) * 0.1;
}

function searchCards(query, pool) {
  return pool
    .map((c) => ({ c, s: matchScore(query, c.display) }))
    .filter((x) => x.s > -1)
    .sort((a, b) => b.s - a.s)
    .slice(0, 8)
    .map((x) => x.c);
}

/* ---------------------------------------------------------------------
 * Row validation (Task 1). Pure functions, no DOM — run under node too.
 *
 * (a) AGE SPAN is a hard block: the civil deck is dealt one age at a
 *     time, so the row can only ever hold one age, or two adjacent ages
 *     during a changeover. distinct ages mapped to A=0,I=1,II=2,III=3;
 *     max-min > 1 is impossible and must not be advised on.
 * (b) DUPLICATES > 2 copies of the same card id is a WARNING only — two
 *     copies can legitimately coexist, so this cannot be a hard block,
 *     and there is no per-card copy limit in cards.json to check against.
 * ------------------------------------------------------------------- */
const AGE_ORDER = ['A', 'I', 'II', 'III'];

function validateRow(row, cardsById) {
  const present = [];
  row.forEach((id, i) => {
    if (!id) return;
    const card = cardsById.get(id);
    if (!card) return; // unknown id: nothing to validate against, ignore
    present.push({ i, id, card });
  });

  const result = { ageIssue: null, dupIssue: null };

  // --- (a) age span ---
  if (present.length) {
    const values = [...new Set(present.map((p) => AGE_ORDER.indexOf(p.card.age)))].filter((v) => v >= 0);
    if (values.length) {
      const min = Math.min(...values);
      const max = Math.max(...values);
      if (max - min > 1) {
        const counts = new Map();
        present.forEach((p) => {
          const v = AGE_ORDER.indexOf(p.card.age);
          counts.set(v, (counts.get(v) || 0) + 1);
        });
        // Majority age = the age holding the most cards in the row (ties -> lowest age wins).
        // Everything more than one age-step away from it is the outlier group.
        let majority = min;
        let majorityCount = -1;
        [...counts.keys()].sort((a, b) => a - b).forEach((v) => {
          const c = counts.get(v);
          if (c > majorityCount) { majorityCount = c; majority = v; }
        });
        const outliers = present.filter((p) => Math.abs(AGE_ORDER.indexOf(p.card.age) - majority) > 1);
        const outlierNames = [...new Set(outliers.map((p) => p.card.display))];
        const outlierAges = [...new Set(outliers.map((p) => p.card.age))];
        const majorityAge = AGE_ORDER[majority];
        result.ageIssue = {
          message: `${outlierNames.join(', ')} ${outlierNames.length > 1 ? 'are' : 'is'} age ${outlierAges.join('/')} ` +
            `but the rest of the row is age ${majorityAge} — the row can only hold one age or two adjacent ages.`,
          outliers: outliers.map((p) => p.i),
        };
      }
    }
  }

  // --- (b) duplicates ---
  const idCounts = new Map();
  present.forEach((p) => idCounts.set(p.id, (idCounts.get(p.id) || 0) + 1));
  const dups = [...idCounts.entries()]
    .filter(([, n]) => n > 2)
    .map(([id, n]) => ({ id, n, display: cardsById.get(id).display }));
  if (dups.length) {
    result.dupIssue = {
      message: dups.map((d) => `${d.display} ×${d.n}`).join(', ') +
        (dups.length > 1 ? ' appear more than twice in the row.' : ' appears more than twice in the row.'),
      dups,
    };
  }

  return result;
}

/* ---------------------------------------------------------------------
 * State + persistence
 *
 * v2: row removal is now expressed as "which slots are gone" instead of
 * "how many fell off the left" (dropCount could not express the rival
 * taking a card out of the middle, which happens constantly). Key is
 * bumped so a stale v1 save is never misread as the new shape.
 * ------------------------------------------------------------------- */
const STORAGE_KEY = 'ttaapp_state_v2';

function freshFlow() {
  return {
    step: 'gone', // gone | new | rival | military | blocked | dupConfirm | thinking | advice | fullrow
    goneSlots: [],
    newCards: [],
    newIndex: 0,
    rivalStr: 0,
    rivalCulture: 0,
    militaryDrafted: false,
    militaryCards: [],
    dupConfirmed: false,
    candidateRow: null,
    blockMessage: '',
    dupMessage: '',
    fullRowDraft: null,
    fullRowCursor: 0,
    fullRowScanStatus: '',
  };
}

function freshState() {
  return {
    v: 2,
    row: new Array(13).fill(null),
    rival: { str: 0, culture: 0 },
    seed: Math.floor(Math.random() * 1e9),
    wasmState: null,
    moves: [],
    positionText: '',
    history: [],
    flow: freshFlow(),
  };
}

let state = null;

function save() {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state)); } catch (e) { /* ignore quota errors */ }
}

function load() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || parsed.v !== 2 || !parsed.flow) return null; // reject anything not shaped like v2
    return parsed;
  } catch (e) { return null; }
}

function snapshotForUndo() {
  const clone = JSON.parse(JSON.stringify(state));
  delete clone.history;
  state.history.push(clone);
  if (state.history.length > 25) state.history.shift();
}

function undo() {
  if (!state.history.length) return;
  const prevHistory = state.history;
  state = state.history.pop();
  state.history = prevHistory.slice(0, prevHistory.length - 1);
  save();
  renderAll();
}

/* ---------------------------------------------------------------------
 * DOM refs — resolved lazily in cacheDom() (called from boot), never at
 * module load time, so this file can be `require()`d under node for the
 * pure functions above without touching `document`.
 * ------------------------------------------------------------------- */
const $ = (id) => document.getElementById(id);
const el = {};

function cacheDom() {
  el.banner = $('fakeBanner');
  el.stats = $('stats');
  el.handToggleBtn = $('handToggleBtn');
  el.undoBtn = $('undoBtn');
  el.newGameBtn = $('newGameBtn');
  el.stepArea = $('stepArea');
  el.handPanel = $('handPanel');
  el.handChips = $('handChips');
  el.handCloseBtn = $('handCloseBtn');
}

/* ---------------------------------------------------------------------
 * Small DOM builder helpers
 * ------------------------------------------------------------------- */
function makeBtn(label, cls, onClick) {
  const b = document.createElement('button');
  b.className = 'btn ' + cls;
  b.textContent = label;
  b.addEventListener('click', onClick);
  return b;
}

function makeStepper(label, value, onChange) {
  const wrap = document.createElement('label');
  wrap.className = 'stepperLabel';
  wrap.textContent = label;
  const stepper = document.createElement('div');
  stepper.className = 'stepper';
  stepper.appendChild(makeBtn('−', 'stepbtn', () => onChange(Math.max(0, value - 1))));
  const span = document.createElement('span');
  span.textContent = value;
  stepper.appendChild(span);
  stepper.appendChild(makeBtn('+', 'stepbtn', () => onChange(value + 1)));
  wrap.appendChild(stepper);
  return wrap;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function slotCostLabel(i) {
  return i < 5 ? '1 CA' : i < 9 ? '2 CA' : '3 CA';
}

/* ---------------------------------------------------------------------
 * Top chrome (always visible) + hand panel (toggle)
 * ------------------------------------------------------------------- */
function renderAll() {
  el.banner.classList.toggle('hidden', !Engine.usingStub);
  el.stats.textContent = state.positionText || 'round – · age – · CA – MA – food – res – sci –';
  renderHandPanel();
  el.stepArea.innerHTML = '';
  renderStep(el.stepArea);
  save();
}

/* The engine already tracks the hand: it auto-plays the whole turn, and the
   board dump it hands back in `state` carries the surviving cards. So the hand
   is READ from that dump rather than mirrored in app state -- a second copy
   here could only ever drift out of step with the one the advisor reasons
   over, and there is nothing the user would have to remember to tap. */
function handFromEngine() {
  const line = (state.wasmState || '').split('\n').find((l) => l.startsWith('p0 hand '));
  if (!line) return { civil: [], military: [] };
  const [civilTxt, milTxt] = line.slice('p0 hand '.length).split('|');
  const names = (t) => (t || '').split(',').map((s) => s.trim()).filter(Boolean);
  return { civil: names(civilTxt), military: names(milTxt) };
}

function renderHandPanel() {
  el.handChips.innerHTML = '';
  const hand = handFromEngine();
  const addGroup = (names, kind) => {
    names.forEach((name) => {
      const chip = document.createElement('div');
      chip.className = 'chip' + (kind === 'military' ? ' mil' : '');
      chip.textContent = name;
      el.handChips.appendChild(chip);
    });
  };
  addGroup(hand.civil, 'civil');
  addGroup(hand.military, 'military');
  if (!hand.civil.length && !hand.military.length) {
    const empty = document.createElement('div');
    empty.className = 'stepSub';
    empty.textContent = 'nothing in hand';
    el.handChips.appendChild(empty);
  }
}

/* ---------------------------------------------------------------------
 * Step router (Task 2): exactly one question on screen at a time.
 * ------------------------------------------------------------------- */
function renderStep(container) {
  switch (state.flow.step) {
    case 'gone': return renderGoneStep(container);
    case 'new': return renderNewStep(container);
    case 'rival': return renderRivalStep(container);
    case 'military': return renderMilitaryStep(container);
    case 'blocked': return renderBlockedStep(container);
    case 'dupConfirm': return renderDupConfirmStep(container);
    case 'thinking': return renderThinkingStep(container);
    case 'advice': return renderAdviceStep(container);
    case 'fullrow': return renderFullRowStep(container);
    default: return renderGoneStep(container);
  }
}

// STEP 1 — mark which of the 13 slots are gone (taken by anyone, or swept
// off at an age change). Replaces the old "N fell off the left" stepper,
// which could not express a card leaving from the middle of the row.
function renderGoneStep(container) {
  container.appendChild(makeTitle('Tap the cards that are gone.'));
  container.appendChild(makeSub('Rival took one, you took one, or the age changed and it aged off — doesn\'t matter which.'));

  const grid = document.createElement('div');
  grid.className = 'slotGrid';
  state.row.forEach((id, i) => {
    const div = document.createElement('div');
    const isGone = state.flow.goneSlots.includes(i);
    div.className = 'slotChip' + (id ? '' : ' empty') + (isGone ? ' gone' : '');
    div.innerHTML = `<span class="slotNum">slot ${i + 1} · ${slotCostLabel(i)}</span><span>${cardDisplay(id)}</span>`;
    div.addEventListener('click', () => {
      const idx = state.flow.goneSlots.indexOf(i);
      if (idx === -1) state.flow.goneSlots.push(i); else state.flow.goneSlots.splice(idx, 1);
      renderAll();
    });
    grid.appendChild(div);
  });
  container.appendChild(grid);

  const goneCount = state.flow.goneSlots.length;
  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn(goneCount === 0 ? 'Nothing left the row →' : `Continue (${goneCount} gone) →`, 'primary big', () => {
    if (goneCount === 0) {
      goToRival();
    } else {
      state.flow.newCards = new Array(goneCount).fill(null);
      state.flow.newIndex = 0;
      state.flow.step = 'new';
      renderAll();
    }
  }));
  container.appendChild(btnRow);

  const escRow = document.createElement('div');
  escRow.className = 'btnRow';
  escRow.appendChild(makeBtn('Re-enter whole row', 'small', () => openFullRow()));
  container.appendChild(escRow);
}

// STEP 2 — name the N new cards, one at a time. Skipped entirely if N is 0
// (handled by the "Nothing left the row" branch above never routing here).
function renderNewStep(container) {
  const N = state.flow.newCards.length;
  const i = state.flow.newIndex;

  container.appendChild(makeTitle(`New card ${i + 1} of ${N}`));
  container.appendChild(makeSub('The row always refills to 13 from the right.'));

  const { input, suggest } = makeAutocompleteRow('type card name');
  container.appendChild(input.wrap);
  setupAutocomplete(input.el, suggest, () => ROW_POOL, (c) => {
    state.flow.newCards[i] = c.id;
    advanceNew();
  });
  input.el.focus();

  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn('Slot stayed empty (deck ran dry)', 'small', () => { state.flow.newCards[i] = null; advanceNew(); }));
  container.appendChild(btnRow);

  const navRow = document.createElement('div');
  navRow.className = 'btnRow';
  navRow.appendChild(makeBtn('← Back', 'small', () => {
    if (i > 0) { state.flow.newIndex--; } else { state.flow.step = 'gone'; }
    renderAll();
  }));
  container.appendChild(navRow);
}

function advanceNew() {
  const N = state.flow.newCards.length;
  if (state.flow.newIndex + 1 < N) {
    state.flow.newIndex++;
    renderAll();
  } else {
    goToRival();
  }
}

// STEP 3 — rival strength + culture, prefilled, one tap through if unchanged.
function renderRivalStep(container) {
  container.appendChild(makeTitle('Rival strength and culture.'));

  const unchanged = state.flow.rivalStr === state.rival.str && state.flow.rivalCulture === state.rival.culture;

  const row = document.createElement('div');
  row.className = 'rivalRow';
  row.appendChild(makeStepper('Rival strength', state.flow.rivalStr, (v) => { state.flow.rivalStr = v; renderAll(); }));
  row.appendChild(makeStepper('Rival culture', state.flow.rivalCulture, (v) => { state.flow.rivalCulture = v; renderAll(); }));
  container.appendChild(row);

  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn(unchanged ? 'Unchanged →' : 'Continue →', 'primary big', () => {
    state.rival.str = state.flow.rivalStr;
    state.rival.culture = state.flow.rivalCulture;
    goToMilitary();
  }));
  container.appendChild(btnRow);

  const navRow = document.createElement('div');
  navRow.className = 'btnRow';
  navRow.appendChild(makeBtn('← Back', 'small', () => {
    if (state.flow.newCards.length) { state.flow.newIndex = state.flow.newCards.length - 1; state.flow.step = 'new'; }
    else { state.flow.step = 'gone'; }
    renderAll();
  }));
  container.appendChild(navRow);
}

// STEP 4 — military draw, default No, type-ahead hidden until Yes.
function renderMilitaryStep(container) {
  container.appendChild(makeTitle('Did you draw a military card?'));

  if (!state.flow.militaryDrafted) {
    const btnRow = document.createElement('div');
    btnRow.className = 'btnRow';
    btnRow.appendChild(makeBtn('No', 'primary big', () => proceedToValidation()));
    btnRow.appendChild(makeBtn('Yes', 'big', () => { state.flow.militaryDrafted = true; renderAll(); }));
    container.appendChild(btnRow);
  } else {
    const { input, suggest } = makeAutocompleteRow('military card drawn this turn');
    container.appendChild(input.wrap);
    setupAutocomplete(input.el, suggest, () => MIL_POOL, (c) => { state.flow.militaryCards.push(c.id); renderAll(); });
    input.el.focus();

    const chips = document.createElement('div');
    chips.className = 'chips';
    state.flow.militaryCards.forEach((id, idx) => {
      const chip = document.createElement('div');
      chip.className = 'chip mil';
      chip.innerHTML = `${cardDisplay(id)}<span class="x">×</span>`;
      chip.addEventListener('click', () => { state.flow.militaryCards.splice(idx, 1); renderAll(); });
      chips.appendChild(chip);
    });
    container.appendChild(chips);

    const btnRow = document.createElement('div');
    btnRow.className = 'btnRow';
    btnRow.appendChild(makeBtn('Continue →', 'primary big', () => proceedToValidation()));
    btnRow.appendChild(makeBtn('Actually, no card', 'small', () => { state.flow.militaryDrafted = false; state.flow.militaryCards = []; renderAll(); }));
    container.appendChild(btnRow);
  }

  const navRow = document.createElement('div');
  navRow.className = 'btnRow';
  navRow.appendChild(makeBtn('← Back', 'small', () => { state.flow.step = 'rival'; renderAll(); }));
  container.appendChild(navRow);
}

// Hard block (Task 1a) — cannot proceed until the row is fixed.
function renderBlockedStep(container) {
  container.appendChild(makeTitle('That row is not legal.', 'bad'));
  const msg = document.createElement('div');
  msg.className = 'msgBox bad';
  msg.textContent = state.flow.blockMessage;
  container.appendChild(msg);
  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn('Back to step 1 →', 'primary big', () => { state.flow.step = 'gone'; renderAll(); }));
  container.appendChild(btnRow);
}

// Soft warning (Task 1b) — one confirming tap and it proceeds.
function renderDupConfirmStep(container) {
  container.appendChild(makeTitle('Double check the row.', 'warn'));
  const msg = document.createElement('div');
  msg.className = 'msgBox warn';
  msg.textContent = state.flow.dupMessage;
  container.appendChild(msg);
  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn("Yes, that's right → Advise", 'primary big', () => {
    state.flow.dupConfirmed = true;
    finalizeTurn(state.flow.candidateRow);
  }));
  container.appendChild(btnRow);
  const navRow = document.createElement('div');
  navRow.className = 'btnRow';
  navRow.appendChild(makeBtn('← Back', 'small', () => { state.flow.step = 'military'; renderAll(); }));
  container.appendChild(navRow);
}

function renderThinkingStep(container) {
  container.appendChild(makeTitle('Thinking…'));
}

// Advice, full-screen, nothing else competing for space.
function renderAdviceStep(container) {
  container.appendChild(makeTitle('Advice'));

  const moves = state.moves || [];
  if (!moves.length) {
    container.appendChild(makeSub('no advice yet'));
  } else {
    const top = moves[0];
    const topDiv = document.createElement('div');
    topDiv.className = 'adviceTop';
    topDiv.innerHTML = `<span class="step">1</span>${escapeHtml(top.text)}` +
      (top.detail ? `<span class="detail">${escapeHtml(top.detail)}</span>` : '');
    container.appendChild(topDiv);

    const rest = document.createElement('div');
    rest.className = 'adviceRest';
    moves.slice(1).forEach((m, i) => {
      const div = document.createElement('div');
      div.className = 'move';
      div.innerHTML = `<span class="step">${i + 2}</span>${escapeHtml(m.text)}` +
        (m.detail ? `<span class="detail">${escapeHtml(m.detail)}</span>` : '');
      rest.appendChild(div);
    });
    container.appendChild(rest);
  }

  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn('Start next turn →', 'primary big', () => { resetFlowForNewTurn(); renderAll(); }));
  container.appendChild(btnRow);
}

// Full 13-slot entry — turn 1, or a resync after losing track. Reuses the
// same "card N of 13" one-at-a-time heading as step 2.
function renderFullRowStep(container) {
  const draft = state.flow.fullRowDraft;
  const cursor = state.flow.fullRowCursor;

  container.appendChild(makeTitle(`Card ${cursor + 1} of 13`));
  container.appendChild(makeSub('Enter the row left to right — slot 1 is the cheapest to take.'));

  const grid = document.createElement('div');
  grid.className = 'slotGrid mini';
  draft.forEach((id, i) => {
    const div = document.createElement('div');
    div.className = 'slotChip' + (id ? '' : ' empty') + (i === cursor ? ' current' : '');
    div.innerHTML = `<span class="slotNum">${i + 1}</span><span>${cardDisplay(id)}</span>`;
    div.addEventListener('click', () => { state.flow.fullRowCursor = i; renderAll(); });
    grid.appendChild(div);
  });
  container.appendChild(grid);

  container.appendChild(renderScanControl());
  if (state.flow.fullRowScanStatus) {
    container.appendChild(makeSub(state.flow.fullRowScanStatus));
  }

  const { input, suggest } = makeAutocompleteRow('type card, Enter to place');
  container.appendChild(input.wrap);
  setupAutocomplete(input.el, suggest, () => ROW_POOL, (c) => {
    draft[cursor] = c.id;
    learnSlotIfScanned(cursor, c.id); // every name typed after a scan teaches the table
    fullRowAdvance();
  });
  input.el.focus();

  const btnRow = document.createElement('div');
  btnRow.className = 'btnRow';
  btnRow.appendChild(makeBtn('Slot is empty', 'small', () => { draft[cursor] = null; fullRowAdvance(); }));
  btnRow.appendChild(makeBtn('← Back', 'small', () => { state.flow.fullRowCursor = Math.max(0, cursor - 1); renderAll(); }));
  container.appendChild(btnRow);

  const navRow = document.createElement('div');
  navRow.className = 'btnRow';
  navRow.appendChild(makeBtn('Cancel', 'small', () => { state.flow.step = 'gone'; renderAll(); }));
  navRow.appendChild(makeBtn('Done →', 'primary', () => finishFullRow()));
  container.appendChild(navRow);
}

function fullRowAdvance() {
  const draft = state.flow.fullRowDraft;
  const cursor = state.flow.fullRowCursor;
  if (cursor >= 12) { finishFullRow(); return; }
  const next = draft.findIndex((x, i) => i > cursor && x === null);
  state.flow.fullRowCursor = next !== -1 ? next : cursor + 1;
  renderAll();
}

function finishFullRow() {
  state.row = state.flow.fullRowDraft.slice(0, 13);
  while (state.row.length < 13) state.row.push(null);
  goToRival();
}

function makeTitle(text, cls) {
  const div = document.createElement('div');
  div.className = 'stepTitle' + (cls ? ' ' + cls : '');
  div.textContent = text;
  return div;
}

function makeSub(text) {
  const div = document.createElement('div');
  div.className = 'stepSub';
  div.textContent = text;
  return div;
}

// File-picker entry point for a screenshot scan. A plain <input type=file
// accept=image/*> is the only thing that works on iOS without a dependency
// — no <video>/getUserMedia capture flow, because this app wants a PHOTOS-
// LIBRARY screen capture, not a live camera shot of the glossy screen.
function renderScanControl() {
  const wrap = document.createElement('div');
  wrap.className = 'ocrRow';

  const label = makeSub('Or scan a screenshot of the row:');
  label.style.marginBottom = '4px';
  wrap.appendChild(label);

  const fileInput = document.createElement('input');
  fileInput.type = 'file';
  fileInput.accept = 'image/*';
  fileInput.className = 'ocrFileInput';
  fileInput.addEventListener('change', async (e) => {
    const file = e.target.files && e.target.files[0];
    if (!file) return;
    state.flow.fullRowScanStatus = 'Scanning…';
    renderAll();
    try {
      const result = await ocrScanSeam(file);
      let filled = 0;
      result.row.forEach((id, i) => {
        if (id) { state.flow.fullRowDraft[i] = id; filled++; }
      });
      const firstUnknown = state.flow.fullRowDraft.findIndex((x) => x === null);
      state.flow.fullRowCursor = firstUnknown === -1 ? state.flow.fullRowCursor : firstUnknown;
      state.flow.fullRowScanStatus = filled === 13
        ? 'Recognised all 13 of 13 from the screenshot.'
        : `Recognised ${filled} of 13 from the screenshot — type the rest; each one you type teaches it for next time.`;
    } catch (err) {
      state.flow.fullRowScanStatus = err && err.code === 'NO_SLOTS'
        ? "Couldn't find the row in that image — type it in instead."
        : 'Scan failed: ' + (err && err.message ? err.message : String(err));
    }
    renderAll();
  });
  wrap.appendChild(fileInput);
  return wrap;
}

function makeAutocompleteRow(placeholder) {
  const wrap = document.createElement('div');
  wrap.className = 'addRow';
  const inputEl = document.createElement('input');
  inputEl.type = 'text';
  inputEl.placeholder = placeholder;
  inputEl.autocomplete = 'off';
  inputEl.autocapitalize = 'off';
  inputEl.autocorrect = 'off';
  inputEl.spellcheck = false;
  const suggest = document.createElement('div');
  suggest.className = 'suggest hidden';
  wrap.appendChild(inputEl);
  wrap.appendChild(suggest);
  return { input: { el: inputEl, wrap }, suggest };
}

/* ---------------------------------------------------------------------
 * Step transitions between the four questions
 * ------------------------------------------------------------------- */
function goToRival() {
  state.flow.rivalStr = state.rival.str;
  state.flow.rivalCulture = state.rival.culture;
  state.flow.step = 'rival';
  renderAll();
}

function goToMilitary() {
  state.flow.step = 'military';
  renderAll();
}

function resetFlowForNewTurn() {
  const step = freshFlow();
  Object.assign(state.flow, step);
}

/* ---------------------------------------------------------------------
 * Autocomplete wiring (unchanged contract; called fresh per render since
 * step screens rebuild their DOM each time)
 * ------------------------------------------------------------------- */
function setupAutocomplete(input, suggestEl, poolGetter, onCommit) {
  let current = [];
  function renderSuggest() {
    const q = input.value.trim();
    current = q ? searchCards(q, poolGetter()) : [];
    suggestEl.innerHTML = '';
    suggestEl.classList.toggle('hidden', current.length === 0);
    current.forEach((c) => {
      const div = document.createElement('div');
      div.className = 'opt';
      div.innerHTML = `${c.display}<small>${c.age} · ${c.type}</small>`;
      div.addEventListener('mousedown', (e) => { e.preventDefault(); commit(c); });
      suggestEl.appendChild(div);
    });
  }
  function commit(card) {
    onCommit(card);
    input.value = '';
    current = [];
    suggestEl.innerHTML = '';
    suggestEl.classList.add('hidden');
    input.focus();
  }
  input.addEventListener('input', renderSuggest);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && current.length) { e.preventDefault(); commit(current[0]); }
    if (e.key === 'Escape') { input.value = ''; renderSuggest(); }
  });
  input.addEventListener('blur', () => setTimeout(() => suggestEl.classList.add('hidden'), 150));
}

/* ---------------------------------------------------------------------
 * Row assembly + validation gate + advisor call
 * ------------------------------------------------------------------- */
function buildRowLine(row) {
  const slots = row.slice(0, 13);
  while (slots.length < 13) slots.push(null);
  return 'row ' + slots.map(cardDisplay).join(', ');
}

// Cards leave from anywhere (gone slots), the rest slide left, and exactly
// as many new cards arrive on the right to refill to 13.
function computeNewRowFromFlow() {
  const kept = state.row.filter((_, i) => !state.flow.goneSlots.includes(i));
  const combined = kept.concat(state.flow.newCards);
  while (combined.length < 13) combined.push(null);
  return combined.slice(0, 13);
}

function proceedToValidation() {
  const newRow = computeNewRowFromFlow();
  state.flow.candidateRow = newRow;
  const result = validateRow(newRow, CARDS_BY_ID);

  if (result.ageIssue) {
    state.flow.step = 'blocked';
    state.flow.blockMessage = result.ageIssue.message;
    renderAll();
    return;
  }
  if (result.dupIssue && !state.flow.dupConfirmed) {
    state.flow.step = 'dupConfirm';
    state.flow.dupMessage = result.dupIssue.message;
    renderAll();
    return;
  }
  finalizeTurn(newRow);
}

async function finalizeTurn(newRow) {
  state.flow.step = 'thinking';
  renderAll();

  snapshotForUndo();
  state.row = newRow.slice(0, 13);
  while (state.row.length < 13) state.row.push(null);

  const lines = [buildRowLine(state.row)];
  lines.push(`p1 str=${state.rival.str} c=${state.rival.culture}`);
  // A military draw is the one thing the engine cannot deduce -- the card comes
  // off a face-down deck. `p0 hand` REPLACES both halves of the hand, so the
  // civil side has to be re-stated verbatim from the engine's own dump or the
  // line silently empties a hand the advisor was counting on.
  if (state.flow.militaryCards.length) {
    const hand = handFromEngine();
    const military = hand.military.concat(state.flow.militaryCards.map(cardDisplay));
    lines.push(`p0 hand ${hand.civil.join(', ')} | ${military.join(', ')}`);
  }

  const request = {
    players: 2,
    seat: 0,
    seed: state.seed,
    state: state.wasmState,
    lines,
  };

  try {
    const resp = await Engine.advise(request);
    if (resp && resp.ok) {
      state.wasmState = resp.state;
      state.moves = resp.moves || [];
      state.positionText = formatPosition(resp.position) || state.positionText;
    } else {
      state.moves = [{ text: 'advisor error: ' + (resp && resp.error ? resp.error : 'unknown'), score: 0, detail: '' }];
    }
  } catch (e) {
    state.moves = [{ text: 'advisor call failed: ' + e.message, score: 0, detail: '' }];
  }

  state.flow.step = 'advice';
  renderAll();
}

// The engine reports the position as fields, so there is nothing to parse.
// It is the position at the START of the turn -- what the listed moves are
// about to spend.
function formatPosition(p) {
  if (!p) return '';
  return `round ${p.round}, age ${p.age} · CA ${p.civil_actions}, MA ${p.military_actions}, ` +
    `food ${p.food}, res ${p.resources}, sci ${p.science}`;
}

/* ---------------------------------------------------------------------
 * Full-row entry (turn 1 / resync escape hatch)
 * ------------------------------------------------------------------- */
function openFullRow() {
  lastScan = null; // a new full-row session invalidates any prior scan's slot geometry
  state.flow.step = 'fullrow';
  state.flow.fullRowDraft = state.row.slice(0, 13);
  while (state.flow.fullRowDraft.length < 13) state.flow.fullRowDraft.push(null);
  const cursor = state.flow.fullRowDraft.findIndex((x) => x === null);
  state.flow.fullRowCursor = cursor === -1 ? 0 : cursor;
  renderAll();
}

/* ---------------------------------------------------------------------
 * Wire everything up — only the always-present chrome buttons; every
 * per-step control is wired inline when that step's DOM is built, since
 * the step screens are rebuilt from scratch on every render.
 * ------------------------------------------------------------------- */
function wireUI() {
  el.undoBtn.addEventListener('click', undo);

  el.newGameBtn.addEventListener('click', () => {
    if (!confirm('Start a new game? This clears the current session.')) return;
    state = freshState();
    save();
    openFullRow();
  });

  el.handToggleBtn.addEventListener('click', () => el.handPanel.classList.toggle('hidden'));
  el.handCloseBtn.addEventListener('click', () => el.handPanel.classList.add('hidden'));
}

/* ---------------------------------------------------------------------
 * Boot
 * ------------------------------------------------------------------- */
async function boot() {
  cacheDom();
  await loadCards();
  await Engine.init();
  const restored = load();
  state = restored || freshState();
  wireUI();
  renderAll();
  const hasAnyCard = state.row.some((x) => x);
  const freshBoot = !hasAnyCard && !state.moves.length && state.flow.step === 'gone' && !state.flow.goneSlots.length;
  if (freshBoot) openFullRow();
}

/* ---------------------------------------------------------------------
 * Under node (verification / fixtures), export the pure functions and
 * skip boot() entirely — there is no document to attach to. In a real
 * browser `module` is undefined and this just calls boot() as before.
 * ------------------------------------------------------------------- */
if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    validateRow, AGE_ORDER, matchScore, searchCards,
    findRowSlots, hashSlot, slotToGrid, hashGrid, HASH_GRID, HASH_BUCKETS,
  };
} else {
  boot();
}
