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

  // ---- 3. build slot (high) / border-or-gap (low) runs ----
  let runs = buildRuns(colTex, colThresh, width);
  if (!runs.length) return null;

  // drop spuriously narrow "high" runs (noise inside a gap/border) and
  // re-merge the low runs that were separated only by that noise
  let highWidths = runs.filter(r => !r.low).map(r => r.end - r.start + 1);
  if (!highWidths.length) return null;
  highWidths.sort((a, b) => a - b);
  const medianHighWidth = highWidths[Math.floor(highWidths.length / 2)];
  runs = runs.filter(r => r.low || (r.end - r.start + 1) >= medianHighWidth * 0.3);
  runs = mergeAdjacentLow(runs);

  highWidths = runs.filter(r => !r.low).map(r => r.end - r.start + 1).sort((a, b) => a - b);
  if (!highWidths.length) return null;
  const cardW = highWidths[Math.floor(highWidths.length / 2)];
  // A card border is the narrow class of low run; the group gaps and any
  // empty-slot span are the wide class, so the median is taken over the
  // narrow class alone or a single wide gap would drag it up.
  const borderWidths = runs
    .filter(r => r.low && (r.end - r.start + 1) < cardW * 0.5)
    .map(r => r.end - r.start + 1)
    .sort((a, b) => a - b);
  const border = borderWidths.length
    ? borderWidths[Math.floor(borderWidths.length / 2)]
    : Math.max(1, Math.round(cardW * 0.04));
  const pitch = cardW + border;

  // An EMPTY row position has no card art in it, so its columns carry no
  // vertical texture and it reads as one wide low run merged with the
  // borders either side of it — 12 cards instead of 13, and the whole scan
  // fails. A low run is therefore measured rather than trusted: subtract
  // one border and it holds round(rest / pitch) card-width spans, which is
  // 0 for a border, 0 for a group gap, and k for k empty positions.
  // Empty slots in the LEADING run sit against the first real card on
  // their right (the rest of that run is the outer margin); everywhere
  // else they start one border after the card on their left.
  const slotRuns = [];
  runs.forEach((r, idx) => {
    if (!r.low) { slotRuns.push({ start: r.start, end: r.end, empty: false }); return; }
    const span = r.end - r.start + 1;
    if (span < cardW * 0.6) return;
    const k = Math.round((span - border) / pitch);
    for (let i = 0; i < k; i++) {
      if (idx === 0) {
        const end = r.end - border - i * pitch;
        slotRuns.push({ start: end - cardW + 1, end, empty: true });
      } else {
        const start = r.start + border + i * pitch;
        slotRuns.push({ start, end: start + cardW - 1, empty: true });
      }
    }
  });
  slotRuns.sort((a, b) => a.start - b.start);
  if (slotRuns.length !== 13) return null;

  // ---- 4. classify the 12 boundaries into 2 group gaps + 10 card borders ----
  const gapWidths = [];
  for (let i = 1; i < slotRuns.length; i++) {
    gapWidths.push(slotRuns[i].start - slotRuns[i - 1].end - 1);
  }
  if (gapWidths.some(g => g < 0)) return null;
  const sortedWidths = [...gapWidths].sort((a, b) => b - a);
  const groupGapThresh = (sortedWidths[1] + sortedWidths[2]) / 2;

  const groups = [];
  let cur = 1;
  for (let i = 0; i < gapWidths.length; i++) {
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
    empty: r.empty,
  }));

  return { slots, groups };
}

/* ---------------------------------------------------------------------
 * OCR — reads the card NAME printed inside each slot, with zero prior
 * teaching, then resolves it against the 236 known card names in
 * cards.json. A slot OCR can't resolve comes back unknown for the user to
 * type — there is deliberately no second guessing pass behind it.
 *
 * Calibrated empirically against the one real capture this app has
 * (ipad_screenshot_2360x1640_2026-08-23.png) — see /private/tmp/
 * appocr2_notes.txt for the numbers and why. Every geometric constant
 * below is a FRACTION of slot width/height, never an absolute pixel
 * offset, so it survives a different screen resolution the same way
 * findRowSlots does.
 *
 * Text vs. decoration: the card frames are painted in saturated colour
 * (green scrollwork, gold border, etc.) even where they're visually
 * "dark"; the printed name is desaturated near-black. So "ink" is
 * luma < 110 AND (max channel - min channel) < 28 — a colour-blind
 * darkness test would also catch the ornate leader-card frames and
 * corrupt the row search.
 * ------------------------------------------------------------------- */
function isInk(r, g, b) {
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b);
  const luma = 0.299 * r + 0.587 * g + 0.114 * b;
  return luma < 110 && (mx - mn) < 28;
}

// Name band search zone, as a fraction of slot height: above 0.20 is the
// card's top badge/frame (on leader cards, the age-roman-numeral badge
// letter renders in a desaturated grey that would otherwise false-positive
// as "ink"); below 0.46 is where art/icons start. Empirically every one of
// the 13 known cards' name text (1 or 2 lines) falls inside this band.
const OCR_LINE_Y_LO = 0.20, OCR_LINE_Y_HI = 0.46;
const OCR_LINE_X_LO = 0.05, OCR_LINE_X_HI = 0.95;
const OCR_ROW_INK_FRAC = 0.05; // row counts as "text" once >5% of its pixels are ink
// px of non-text row allowed inside one run before it splits in two. Small
// on purpose: a second capture has two real text lines ("Hanging Gardens"
// on a purple wonder frame) sitting only 2 rows apart, and the old value
// (3) silently fused them into one line, corrupting every glyph box after
// it (see appatlas_notes.txt). A small ascender/descender tip (e.g. the
// top of "P"/"d" in "Pyramids") can ALSO sit this close to its own line's
// main body, separated by its own >1-row anti-alias gap -- so splitting
// on every gap this small would wrongly cut real single-line names too.
// The fix is two-stage: split eagerly here (correctly separating the two
// real lines), then re-fuse any resulting run too short to be a real line
// back into its nearest surviving neighbour below, so an ascender tip
// still ends up folded into the same bounding box as before rather than
// truncating it or being discarded outright.
const OCR_ROW_GAP_MERGE = 1;
const OCR_MIN_LINE_FRAC = 0.02; // drop stray 1-2px specks that aren't a real text line

function findTextLines(imageData, slot) {
  const { data, width, height } = imageData;
  const x0 = Math.round(slot.x * width), y0 = Math.round(slot.y * height);
  const w = Math.round(slot.w * width), h = Math.round(slot.h * height);
  const xm0 = Math.round(w * OCR_LINE_X_LO), xm1 = Math.round(w * OCR_LINE_X_HI);
  const yLo = Math.round(h * OCR_LINE_Y_LO), yHi = Math.round(h * OCR_LINE_Y_HI);
  const rows = [];
  for (let y = yLo; y < yHi; y++) {
    let cnt = 0;
    for (let x = xm0; x < xm1; x++) {
      const i = ((y0 + y) * width + (x0 + x)) * 4;
      if (isInk(data[i], data[i + 1], data[i + 2])) cnt++;
    }
    rows.push({ y, f: cnt / Math.max(1, xm1 - xm0) });
  }
  const runs = [];
  let cur = null, gapRun = 0;
  for (const r of rows) {
    if (r.f > OCR_ROW_INK_FRAC) {
      if (!cur) cur = { start: r.y, end: r.y };
      else cur.end = r.y;
      gapRun = 0;
    } else if (cur) {
      gapRun++;
      if (gapRun > OCR_ROW_GAP_MERGE) { runs.push(cur); cur = null; gapRun = 0; }
    }
  }
  if (cur) runs.push(cur);

  // Fold any run shorter than a real line's floor height into whichever
  // neighbouring run (previous already-kept, or next not-yet-seen) sits
  // closer in y -- an ascender/descender tip belongs to that line's own
  // bounding box, not a discarded speck and not a phantom third line.
  // An isolated short run with no neighbour on either side (both gaps
  // Infinity) falls through unmerged and is dropped by the final filter,
  // exactly as before this two-stage split was added.
  const minLineRows = h * OCR_MIN_LINE_FRAC;
  const kept = [];
  for (let i = 0; i < runs.length; i++) {
    const r = runs[i];
    if ((r.end - r.start + 1) < minLineRows) {
      const distPrev = kept.length ? r.start - kept[kept.length - 1].end : Infinity;
      const distNext = (i + 1 < runs.length) ? runs[i + 1].start - r.end : Infinity;
      if (distPrev <= distNext && kept.length) {
        kept[kept.length - 1].end = r.end;
        continue;
      }
      if (distNext < Infinity) {
        runs[i + 1].start = r.start;
        continue;
      }
      // no neighbour at all -- keep as-is, the size filter below drops it
    }
    kept.push(r);
  }
  return kept.filter((r) => (r.end - r.start + 1) >= minLineRows);
}

const OCR_GLYPH_X_LO = 0.03, OCR_GLYPH_X_HI = 0.97;
const OCR_MIN_GLYPH_PX = 3; // narrower than this is anti-alias noise, not a stroke

// Per-column ink pixel count across one text line, slot-relative x index
// starting at OCR_GLYPH_X_LO — the shared input for both the zero-ink cut
// and the fusion-valley search below, so they never see different data.
function lineColInk(imageData, slot, lineRun) {
  const { data, width, height } = imageData;
  const x0 = Math.round(slot.x * width), y0 = Math.round(slot.y * height);
  const w = Math.round(slot.w * width);
  const xm0 = Math.round(w * OCR_GLYPH_X_LO), xm1 = Math.round(w * OCR_GLYPH_X_HI);
  const colInk = [];
  for (let x = xm0; x < xm1; x++) {
    let cnt = 0;
    for (let y = lineRun.start; y <= lineRun.end; y++) {
      const i = ((y0 + y) * width + (x0 + x)) * 4;
      if (isInk(data[i], data[i + 1], data[i + 2])) cnt++;
    }
    colInk.push(cnt);
  }
  return { colInk, xm0 };
}

// Cut on zero-ink columns only — the coarse pass. Any column with zero ink
// pixels ends the current glyph; a faint anti-alias bridge between two
// touching letters never reaches zero, so this alone under-segments (see
// splitFusedBox below, which is what actually separates those).
function zeroInkBoxes(colInk) {
  const boxes = [];
  let cur = null;
  for (let i = 0; i < colInk.length; i++) {
    if (colInk[i] > 0) {
      if (!cur) cur = { start: i, end: i };
      else cur.end = i;
    } else if (cur) {
      boxes.push(cur);
      cur = null;
    }
  }
  if (cur) boxes.push(cur);
  return boxes.filter((b) => b.end - b.start + 1 >= OCR_MIN_GLYPH_PX);
}

function median(nums) {
  if (!nums.length) return 0;
  const s = [...nums].sort((a, b) => a - b);
  const n = s.length;
  return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2;
}

// A box wider than this multiple of the median glyph width is treated as a
// fusion candidate. The task brief suggested ~1.4x; 1.5x is what this data
// needed — recursion re-applies this same gate to each split-off piece
// (see splitFusedBox), and at 1.4x a lone "u" left over after correctly
// splitting off "r" (width ~1.43x median) still cleared the gate and got
// examined again, occasionally finding a spurious internal valley in "u"
// itself (see OCR_VALLEY_DEPTH_RATIO's note on why depth alone doesn't
// always save it) and wrongly slicing it in two. 1.5x leaves single wide
// letters ("m"/"w"/"u"/capital-width letters) alone once they're down to
// their own natural width, while still catching genuine 2+-letter runs
// (which start closer to 2x a single letter's width to begin with).
const OCR_FUSION_RATIO = 1.5;
// A candidate split column must dip to at most this fraction of the lower
// of its two surrounding in-box peaks to count as a real inter-letter
// bridge rather than a letter's own internal shape (e.g. the low-but-not-
// near-zero column count through the crossbar of "m"/"w", or the middle of
// a rounded "o" — see appatlas_notes.txt's threshold-sweep dead end, which
// is exactly the failure mode this ratio (checked against the LOCAL peaks
// inside the candidate box, not a global column-count threshold) avoids).
// Picked by dumping every local-minimum ratio inside every fusion-
// candidate box in both captures: genuine touching-letter bridges cluster
// tightly at 0.07-0.09 of their flanking peaks (a near-literal anti-alias
// sliver against full stroke height); the saddle where "m"/"w"'s arches
// meet their legs measured 0.17-0.27 (first tried 0.3, which let those
// through and wrongly split "m" in "Homer"/"Hammurabi" — see notes.txt).
// 0.15 sits in the gap between the two clusters.
const OCR_VALLEY_DEPTH_RATIO = 0.15;

// Find the best column to cut a fusion-candidate box at, or null if no
// column qualifies as a real valley. A qualifying column must be a local
// minimum in colInk AND dip below OCR_VALLEY_DEPTH_RATIO of the smaller of
// its two in-box flanking peaks (see above). Returns the LEFTMOST
// qualifying column, not the deepest or the one nearest a medianWidth
// multiple: a real fused pair can have two equally-deep valleys (e.g. "u"'s
// own open counter can dip as low as a genuine letter gap, immediately
// after the real r/u boundary in "Frugality" — see notes.txt). Cutting at
// the first one leaves "u" intact and under the fusion-ratio gate, so it's
// never examined again; a nearest-to-median scoring tried first picked the
// later, wrong valley whenever it happened to land marginally closer to
// medianWidth, splitting "u" itself instead of the r/u boundary.
function bestValley(colInk, box) {
  let peakL = -Infinity;
  for (let j = box.start + 1; j < box.end; j++) {
    peakL = Math.max(peakL, colInk[j - 1]);
    if (colInk[j] > colInk[j - 1] || colInk[j] > colInk[j + 1]) continue; // not a local min
    let peakR = -Infinity;
    for (let k = j + 1; k <= box.end; k++) peakR = Math.max(peakR, colInk[k]);
    const peak = Math.min(peakL, peakR);
    if (peak <= 0 || colInk[j] > peak * OCR_VALLEY_DEPTH_RATIO) continue; // near the peak — a real stroke, not a bridge
    return j;
  }
  return null;
}

// Minimum width a split piece must have to be kept — below this it's more
// likely a stray sliver from a bad cut than a real narrow letter ("i"/"l"
// legitimately get down to OCR_MIN_GLYPH_PX, so re-use that same floor).
function splitFusedBox(colInk, box, medianWidth) {
  const width = box.end - box.start + 1;
  if (width <= medianWidth * OCR_FUSION_RATIO) return [box];
  const j = bestValley(colInk, box);
  if (j == null) return [box]; // no acceptable valley — leave fused rather than guess a wrong cut
  const left = { start: box.start, end: j - 1 };
  const right = { start: j, end: box.end };
  if (left.end - left.start + 1 < OCR_MIN_GLYPH_PX || right.end - right.start + 1 < OCR_MIN_GLYPH_PX) return [box];
  return [...splitFusedBox(colInk, left, medianWidth), ...splitFusedBox(colInk, right, medianWidth)];
}

// Cut a text line into per-glyph column boxes: zero-ink gaps first, then a
// shape-aware pass that finds and splits any box left over-wide by an
// anti-alias bridge the zero-ink cut couldn't see (see splitFusedBox).
// medianWidthHint lets a caller with more context (ocrSlotRaw, pooling
// every line in the slot — see there for why) supply a sturdier reference
// width than this one line alone could give; standalone callers (tests,
// the atlas harvester) fall back to this line's own median.
function segmentGlyphs(imageData, slot, lineRun, medianWidthHint) {
  const { colInk, xm0 } = lineColInk(imageData, slot, lineRun);
  const boxes = zeroInkBoxes(colInk);
  const medianWidth = medianWidthHint != null
    ? medianWidthHint
    : median(boxes.map((b) => b.end - b.start + 1));
  if (!medianWidth) return boxes.map((b) => ({ x0: xm0 + b.start, x1: xm0 + b.end }));
  return boxes
    .flatMap((b) => splitFusedBox(colInk, b, medianWidth))
    .map((b) => ({ x0: xm0 + b.start, x1: xm0 + b.end }));
}

// Normalise one glyph box to a fixed GLYPH_W x GLYPH_H grid of ink
// coverage (0-255 per cell, box-averaged) in slot-relative coordinates —
// resampled by area, never by absolute pixel offset, so
// the same letter at a different capture resolution lands on the same
// grid. Padding on all sides keeps stroke tips (serifs, dots) that sit
// right at the glyph's bounding box from being cut off by resampling.
const GLYPH_W = 10, GLYPH_H = 12;
const OCR_GLYPH_PAD_FRAC = 0.15;

function glyphToGrid(imageData, slot, lineRun, box) {
  const { data, width, height } = imageData;
  const x0 = Math.round(slot.x * width), y0 = Math.round(slot.y * height);
  const bw = box.x1 - box.x0 + 1;
  const padX = Math.max(1, bw * OCR_GLYPH_PAD_FRAC);
  const bh = lineRun.end - lineRun.start + 1;
  const padY = Math.max(1, bh * OCR_GLYPH_PAD_FRAC);
  const gx0 = box.x0 - padX, gx1 = box.x1 + 1 + padX;
  const gy0 = lineRun.start - padY, gy1 = lineRun.end + 1 + padY;
  const cellW = (gx1 - gx0) / GLYPH_W, cellH = (gy1 - gy0) / GLYPH_H;
  const grid = new Uint8Array(GLYPH_W * GLYPH_H);
  for (let gy = 0; gy < GLYPH_H; gy++) {
    const sy0 = gy0 + gy * cellH, sy1 = sy0 + cellH;
    for (let gx = 0; gx < GLYPH_W; gx++) {
      const sx0 = gx0 + gx * cellW, sx1 = sx0 + cellW;
      // fraction of the cell's area covered by ink pixels, sampled at
      // native resolution within the cell (at most a few px per cell at
      // this grid size, so a plain loop is plenty fast)
      const px0 = Math.max(0, Math.floor(sx0)), px1 = Math.min(width - x0, Math.ceil(sx1));
      const py0 = Math.max(0, Math.floor(sy0)), py1 = Math.min(height - y0, Math.ceil(sy1));
      let inkPx = 0, totalPx = 0;
      for (let py = py0; py < py1; py++) {
        for (let px = px0; px < px1; px++) {
          const i = ((y0 + py) * width + (x0 + px)) * 4;
          totalPx++;
          if (isInk(data[i], data[i + 1], data[i + 2])) inkPx++;
        }
      }
      grid[gy * GLYPH_W + gx] = totalPx ? Math.round((inkPx / totalPx) * 255) : 0;
    }
  }
  return grid;
}

function gridDistance(a, b) {
  let sum = 0;
  for (let i = 0; i < a.length; i++) { const d = a[i] - b[i]; sum += d * d; }
  return Math.sqrt(sum / a.length);
}

/* ---------------------------------------------------------------------
 * Glyph atlas — vendored, built OFFLINE from the one real capture this
 * app has (see appocr2_notes.txt for the build process and thresholds).
 * Each entry is [label, grid] where grid is GLYPH_W*GLYPH_H ink-coverage
 * bytes (0-255), flattened row-major. label is usually one character but
 * is whatever substring the offline aligner assigned that glyph box when
 * its width didn't cleanly match 1 box-per-letter (e.g. a serif "m"
 * that split into multiple strokes) — that's fine, classification just
 * concatenates whatever labels the nearest-neighbour picks and the fuzzy
 * matcher against cards.json absorbs the noise.
 *
 * Letters the 13 captured names don't contain (Q, V, W, X, Z, most
 * lowercase-only forms, digits, punctuation) are simply absent — an
 * unmatched glyph degrades to '?' (OCR_WILDCARD_DIST below), not a crash
 * or a wrong guess.
 * ------------------------------------------------------------------- */
const OCR_ATLAS = [
  ["S", [0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,43,0,0,0,0,43,191,255,191,191,191,128,43,0,0,142,213,128,43,43,170,255,85,0,0,170,213,128,43,0,43,85,28,43,0,170,255,255,191,64,0,0,0,0,0,0,64,191,255,255,191,128,43,0,0,0,0,43,85,213,255,255,113,0,0,85,43,0,0,43,170,255,142,0,0,170,128,0,0,64,191,255,85,0,0,113,128,85,85,128,170,128,28,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,128,64,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,43,128,213,255,170,85,85,43,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,43,0,128,255,255,128,0,0,0,0,85,0,128,255,255,128,0,0,0,0,0,0,128,255,255,191,128,128,64,0,0,0,43,128,170,170,170,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,128,85,128,170,128,28,0,0,128,255,128,0,64,191,255,128,0,0,170,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,128,255,191,64,64,191,255,128,0,0,28,128,170,128,128,170,128,28,0,0,0,0,0,0,0,0,0,0,0]],
  ["c", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,128,85,128,170,85,0,0,0,170,191,64,0,64,213,128,0,0,128,255,128,0,0,0,43,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,170,43,0,0,0,0,0,0,64,213,255,191,64,64,128,64,0,0,0,85,170,170,128,128,142,43,43,0,0,0,0,0,0,0,0,0,0]],
  ["k", [0,0,0,0,0,0,0,0,0,0,0,85,170,113,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,85,85,255,170,0,43,142,128,57,0,128,85,255,170,0,64,170,64,0,0,0,85,255,170,64,255,85,0,0,0,0,85,255,227,170,255,142,0,0,0,0,85,255,198,43,170,227,85,0,0,64,85,255,170,0,64,213,191,43,0,43,85,170,142,43,43,142,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["P", [0,0,0,0,0,0,0,0,0,0,0,28,85,85,85,85,57,0,0,0,0,85,255,255,255,191,213,128,43,0,0,28,170,255,170,43,142,255,142,0,0,0,128,255,128,0,85,255,170,0,0,0,128,255,128,0,85,255,170,0,0,0,128,255,128,64,170,255,85,0,0,0,128,255,170,128,142,85,28,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,57,128,170,128,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,43,128,170,170,170,170,85,0,0,0,64,191,255,255,191,128,64,0,0,0,0,43,85,85,43,0,0,0,0,0,85,170,170,170,170,170,85,0,0,0,64,191,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,43,128,170,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,0,0,0,0,0,0,0,0,0,0,43,85,128,170,170,85,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,43,85,128,170,170,128,85,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,85,128,128,128,170,128,28,0,0,85,255,191,64,64,191,255,85,0,0,170,255,191,128,128,191,255,170,0,0,170,255,213,170,170,170,170,85,0,0,142,255,170,43,0,0,0,0,0,0,85,255,255,191,64,64,128,43,0,43,28,128,170,170,128,128,128,28,0,0,0,0,0,0,0,0,0,0,0]],
  ["P", [0,0,0,0,0,0,0,0,0,0,0,85,128,128,128,128,85,0,0,0,0,85,191,213,64,64,170,255,85,0,0,0,128,170,0,0,85,255,142,43,0,0,128,170,0,0,85,255,170,128,0,0,128,170,0,0,85,255,113,43,0,0,128,170,0,43,85,128,28,0,0,0,128,170,0,64,0,0,0,0,0,0,128,170,0,0,0,0,0,0,0,0,128,170,0,0,0,0,0,0,0,43,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["y", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,57,85,85,43,0,0,43,57,0,128,170,255,191,64,0,0,128,170,0,43,113,255,128,0,0,0,128,113,0,0,28,170,213,85,0,43,85,28,0,0,0,128,255,128,0,128,128,0,0,0,0,43,170,213,85,170,85,0,0,0,0,0,64,191,128,128,0,0,0,0,0,0,0,64,128,0,0,0,0,0,0,0,0,85,85,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,43,0,43,43,0,0,0,64,191,255,128,128,191,191,64,0,0,0,85,213,170,128,85,85,43,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,85,0,0,128,255,255,0,0,0,0,64,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,28,0,0,64,0,43,64,128,128,191,128,0,0,43,0,28,0,0,0,43,170,85,0,0,0,28,85,43,0,85,227,128,0,0,0,128,191,64,0,64,213,128,0,0,85,227,128,0,0,0,170,128,0,0,64,213,128,0,0,64,213,128,0,0,0,43,128,128,128,64,85,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["m", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,0,64,128,191,255,255,191,128,64,0,0,0,0,43,85,255,255,170,43,0,0,0,0,0,0,255,255,128,0,0,0,0,0,0,0,255,255,128,0,0,0,0,0,0,0,255,255,128,0,0,0,0,0,0,0,255,255,128,0,0,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,0,0,0,0,0,0,64,128,191,255,128,64,0,0,0,0,0,0,128,255,255,170,43,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["d", [0,0,0,0,0,43,85,85,28,0,0,0,0,0,0,64,170,255,85,0,0,0,0,0,0,0,85,255,85,0,0,0,0,57,85,85,142,255,85,0,0,0,64,170,128,128,170,255,85,0,0,28,170,113,0,0,85,255,85,0,0,85,255,85,0,0,85,255,85,0,0,85,255,85,0,0,85,255,85,0,0,113,255,85,0,0,85,255,85,0,0,85,255,128,0,0,128,255,85,0,64,0,64,128,128,128,85,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,64,191,191,128,191,255,128,0,0,0,128,213,85,0,85,170,85,0,0,0,85,213,213,85,43,0,0,0,0,0,0,64,128,255,191,128,64,0,0,43,85,43,0,85,170,255,128,0,0,128,255,128,0,0,128,255,128,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["R", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,43,0,0,0,0,170,255,191,128,213,191,85,0,0,0,113,255,128,0,113,255,170,0,0,0,85,255,128,0,85,255,170,0,85,0,85,255,128,0,85,255,170,0,64,0,85,255,255,255,255,191,43,0,0,0,85,255,170,85,198,213,85,0,0,0,85,255,128,0,113,255,198,28,0,0,85,255,128,0,43,191,255,85,0,0,85,170,128,57,0,85,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,170,170,85,0,0,0,0,128,255,255,255,255,128,0,0,0,0,43,85,85,85,85,43,0,0,0,85,170,170,170,170,170,85,0,0,0,64,191,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,43,128,170,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["c", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,128,128,85,128,170,85,0,0,64,191,255,128,0,128,255,128,0,0,128,255,255,128,0,0,64,64,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,64,191,255,191,128,128,128,64,0,0,0,43,128,170,170,170,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["h", [0,0,0,0,0,0,0,0,0,0,0,85,170,113,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,198,128,128,170,85,0,0,0,85,255,213,128,64,213,191,43,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,170,142,43,43,142,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["L", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,0,0,0,0,0,0,170,255,255,255,0,0,0,0,0,0,57,170,255,170,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,43,57,43,0,0,128,255,128,0,28,170,142,128,0,0,128,255,128,0,85,255,85,128,0,57,128,170,128,85,113,170,57,43,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,128,85,170,170,128,28,85,0,43,128,64,0,191,255,255,85,64,0,0,0,0,64,191,255,255,85,0,43,85,170,128,128,213,255,255,85,0,85,170,255,128,0,128,255,255,85,0,0,170,255,128,0,191,255,255,85,0,0,85,170,128,85,128,128,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,113,170,142,128,128,170,128,28,0,0,128,255,213,128,64,213,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,43,85,170,142,43,43,142,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["d", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,142,170,57,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,28,85,113,85,85,198,255,85,0,0,85,255,128,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,128,255,128,0,64,213,255,85,0,0,28,128,142,85,128,142,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["E", [0,0,0,0,0,0,0,0,0,0,0,85,170,170,128,85,113,128,28,0,0,28,170,198,43,0,57,170,57,0,0,0,128,170,0,43,57,0,0,85,0,0,128,198,85,170,170,0,0,43,0,0,128,198,85,128,142,0,0,0,0,0,128,170,0,0,0,85,113,0,0,57,170,198,85,85,85,170,113,43,0,57,85,85,85,85,85,85,28,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,113,170,142,128,128,170,128,28,0,0,113,255,198,85,43,198,255,85,0,0,85,255,170,0,0,170,255,85,0,85,85,255,170,0,0,170,255,85,0,43,113,255,198,43,43,198,255,113,0,0,57,85,85,43,43,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,113,85,128,142,128,113,43,0,142,255,85,0,43,198,170,57,0,0,142,255,85,0,43,198,128,0,0,0,57,213,142,85,85,28,0,0,0,0,85,255,198,170,170,142,85,28,43,0,113,213,113,85,85,142,255,142,43,0,170,170,28,0,0,113,255,142,0,0,85,170,113,85,85,142,128,28,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,85,43,0,0,0,0,0,85,213,255,255,128,0,0,0,0,0,0,43,85,85,43,0,0,0,0,43,128,170,170,170,85,0,0,0,0,0,43,170,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,43,85,170,255,255,170,85,43,0,0,43,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,113,170,142,128,128,170,128,28,0,0,113,255,198,85,43,198,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,43,113,255,198,43,43,198,255,113,0,43,57,85,85,43,43,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,85,85,128,142,43,0,0,85,227,128,0,0,43,198,128,0,0,128,255,213,170,170,170,227,128,0,0,128,255,128,0,0,0,0,0,0,0,43,170,213,170,170,170,142,43,0,0,0,28,85,85,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,85,128,170,170,85,85,0,43,170,255,213,128,85,128,85,43,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,43,170,255,255,85,43,0,0,43,0,43,85,85,85,85,43,0,0,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,85,43,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,43,85,85,85,43,0,0,0,0,85,170,170,170,170,85,0,0,0,0,43,170,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,43,170,255,255,255,170,85,43,0,0,43,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,128,113,85,128,170,170,113,0,0,170,255,85,0,43,198,170,57,0,0,142,255,85,0,85,198,85,0,0,0,57,170,113,85,85,28,0,0,0,0,85,213,170,170,170,113,85,28,0,0,113,170,85,85,85,142,255,85,0,0,170,128,0,0,0,113,213,57,0,0,85,128,85,85,85,113,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["G", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,128,85,0,0,0,0,128,191,85,0,64,213,85,0,0,57,198,85,0,0,0,113,57,0,0,128,170,0,0,0,0,0,0,0,0,170,170,0,0,0,0,0,0,0,0,170,170,0,0,0,43,142,85,0,0,128,170,0,0,0,0,170,85,0,0,57,198,85,0,0,0,170,85,0,0,0,128,191,43,0,0,170,85,0,0,0,0,64,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,85,85,85,85,57,0,0,0,0,128,191,128,128,191,213,64,0,0,43,198,128,0,0,43,198,128,0,0,128,255,170,128,170,170,170,85,0,0,128,255,128,64,128,128,43,0,0,0,85,227,128,0,0,0,0,0,0,0,0,170,191,128,64,0,85,64,0,0,0,0,64,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,57,43,85,85,43,0,0,0,170,255,213,191,191,255,128,0,0,0,113,255,198,85,43,198,213,57,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,128,128,64,64,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,85,85,43,0,0,0,0,128,255,255,255,255,128,0,0,0,0,64,128,128,64,0,0,0,0,0,43,85,85,85,85,85,43,0,0,0,128,255,255,255,255,255,128,0,0,0,43,170,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,64,128,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,57,0,0,57,85,57,0,0,128,255,170,0,0,128,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,28,170,170,0,0,85,255,142,0,0,0,128,170,0,0,128,255,85,0,64,0,0,85,128,128,43,64,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,64,191,191,128,128,191,128,0,0,0,128,255,128,0,0,85,85,0,0,0,85,213,213,170,85,43,0,0,0,0,0,64,128,128,255,191,64,0,0,0,43,43,0,0,128,213,213,85,0,0,128,128,0,0,64,191,191,64,0,0,0,64,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["H", [0,0,170,85,0,0,113,198,28,0,0,0,170,85,0,0,85,170,0,28,0,0,170,85,0,0,85,170,0,43,0,0,170,142,85,85,142,170,0,0,0,0,170,85,0,0,85,170,0,28,0,0,170,85,0,0,85,170,0,85,0,0,170,85,0,0,85,170,0,85,0,57,142,113,28,28,113,142,57,57,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["an", [21,0,0,0,0,0,0,0,0,0,0,64,106,106,21,128,85,149,43,21,0,64,32,159,0,191,96,159,96,96,0,0,21,191,43,191,64,128,128,128,0,106,85,213,64,191,64,128,128,106,0,191,64,191,64,191,64,128,128,64,0,191,96,223,64,191,64,128,128,128,43,128,106,149,106,149,64,106,106,106,0,0,0,0,0,0,0,0,0,149,0,0,0,0,0,0,0,0,0,159,0,0,0,0,0,0,0,0,0,85,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,43,113,85,128,170,170,113,43,0,43,191,170,0,64,213,191,85,0,0,85,255,170,0,43,198,128,0,0,0,57,213,198,85,170,198,85,0,0,0,0,128,213,128,128,43,0,0,0,0,85,255,170,128,128,128,64,0,0,43,57,213,227,170,170,227,213,85,43,0,113,255,113,0,0,113,255,170,0,0,128,255,128,0,0,128,255,128,0,0,57,170,198,170,128,142,128,28,0,0,0,0,57,85,43,0,0,0,0]],
  ["i", [0,0,0,0,43,43,0,0,0,0,0,43,128,170,170,170,170,85,0,0,0,0,64,128,191,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,85,170,213,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,43,128,170,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,113,170,142,85,128,170,85,0,0,0,128,255,213,64,64,213,191,43,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,170,142,43,43,142,170,85,0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,64,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,43,113,85,128,142,128,113,0,0,43,191,170,0,64,213,191,85,0,0,85,255,170,0,43,198,128,0,0,0,28,128,170,85,170,198,85,0,0,0,0,64,170,128,128,43,0,0,0,0,43,191,213,128,128,128,64,0,0,43,57,213,227,170,170,227,213,85,0,0,113,255,113,0,0,113,255,170,0,0,128,255,128,0,0,128,255,128,0,0,28,128,142,85,85,142,128,28,0,0,0,0,0,0,0,0,0,0,0]],
  ["G", [0,0,0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,128,43,0,0,43,213,191,85,0,128,255,128,64,0,142,227,85,0,0,43,198,85,0,0,170,170,0,0,0,0,85,43,0,0,170,170,0,0,57,85,85,28,0,0,170,170,0,0,113,213,255,113,0,0,170,170,0,0,0,128,255,85,0,0,142,227,85,0,0,128,255,85,0,0,43,213,191,43,0,128,255,85,0,0,0,43,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["ar", [0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,128,64,43,64,43,43,0,43,64,43,85,21,85,85,57,57,85,57,64,0,43,64,170,191,213,213,223,170,191,43,28,21,28,85,198,85,191,142,64,85,113,21,85,149,198,0,191,85,0,142,85,96,213,128,255,0,191,85,0,170,85,128,113,64,198,28,213,85,0,113,85,128,128,96,170,85,255,85,0,85,0,32,128,96,128,43,128,128,32,0,0,0,0,0,0,0,0,0,0,0]],
  ["d", [0,0,0,0,43,85,142,128,28,0,0,0,0,0,128,255,255,255,85,0,0,43,128,43,64,128,255,255,128,0,43,0,43,113,170,213,255,170,28,0,128,43,191,255,191,128,213,128,0,0,43,85,255,170,43,0,170,128,0,85,0,142,255,85,0,0,170,128,0,128,0,170,255,85,0,0,170,128,0,128,0,113,255,113,0,0,170,128,0,85,0,85,255,213,64,64,213,128,0,0,0,0,64,213,191,0,85,128,43,0,0,0,0,57,43,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,0,0,0,0,0,0,0,0,64,0,0,28,85,85,85,85,57,0,0,0,0,128,255,255,191,191,170,0,0,0,85,198,128,85,43,85,198,85,0,0,128,227,170,170,170,170,170,85,0,0,128,255,191,128,128,128,43,0,0,0,85,227,170,43,0,0,0,0,0,0,0,170,255,191,64,0,43,0,0,0,0,43,191,191,128,128,43,0,0,0,0,0,43,43,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,43,128,128,85,0,0,0,0,0,28,85,43,28,142,170,85,0,0,0,85,255,191,128,170,255,170,0,0,85,28,198,170,57,57,213,198,28,0,85,0,170,128,0,0,128,255,85,0,0,0,170,128,0,0,128,255,85,0,0,0,170,128,0,0,128,255,85,0,0,0,170,128,0,0,128,255,85,64,0,43,128,128,85,43,128,128,85,64,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,85,85,85,85,57,0,0,0,0,128,191,128,128,191,170,0,0,0,0,170,170,43,0,43,85,0,0,0,0,142,255,213,170,128,57,0,0,0,0,43,128,191,255,255,213,64,0,0,0,28,0,43,85,170,255,128,0,0,64,170,64,0,0,128,255,128,0,64,64,128,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["J", [0,0,0,0,0,0,0,0,0,0,0,43,128,170,170,170,170,170,85,0,0,0,64,191,255,255,255,191,64,0,0,0,0,128,255,255,255,128,0,43,0,0,0,128,255,255,255,128,0,43,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,43,170,255,255,128,0,0,0,0,85,213,213,170,128,43,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,43,85,0,0,0,0,0,0,0,0,43,64,0,0,0,0,0,0,0,0,0,0,57,85,57,0,85,85,85,28,0,0,113,255,170,0,85,198,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,198,43,85,198,255,85,0,0,43,191,255,191,255,255,255,128,64,0,0,43,85,85,85,113,128,57,43,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,43,85,85,85,170,85,0,0,0,0,43,170,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,64,191,255,255,255,191,128,64,0,0,43,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["iu", [0,64,85,0,0,0,0,0,0,0,0,128,170,0,0,0,0,0,0,0,0,64,85,0,0,0,0,0,0,0,0,64,57,43,85,57,43,85,21,28,0,170,170,43,198,170,85,255,64,142,0,128,170,0,170,170,64,255,64,170,0,128,170,32,213,170,64,255,64,128,0,128,170,0,170,170,64,255,64,28,0,128,170,0,170,198,106,255,64,113,85,159,213,64,170,255,191,213,128,170,57,64,85,43,57,85,64,113,85,57,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,57,0,0,0,85,227,170,85,85,170,170,0,0,0,128,255,191,64,0,64,85,0,0,0,64,213,255,255,255,191,43,0,0,0,0,57,128,170,213,255,170,43,0,0,85,113,0,0,128,255,227,85,0,64,128,213,128,128,191,255,170,0,0,43,43,85,85,85,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["C", [0,0,0,0,0,0,0,0,0,0,0,0,0,57,128,170,170,170,113,0,0,0,64,213,191,128,128,191,170,0,0,57,213,227,85,0,0,128,170,0,0,142,255,170,0,0,0,43,57,0,0,170,255,170,0,0,0,0,0,0,0,170,255,170,0,0,0,0,0,0,0,113,255,198,43,0,0,43,57,43,0,57,213,255,170,0,28,170,170,85,0,0,64,213,255,128,170,255,170,0,0,0,0,57,85,85,85,85,57,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,85,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,128,0,43,85,85,85,85,43,0,0,43,28,128,170,170,170,227,213,57,43,0,0,0,0,0,0,170,255,85,128,0,0,64,170,255,255,255,255,85,128,43,85,213,198,85,85,198,255,85,128,128,142,255,198,43,43,198,255,85,85,128,85,255,255,191,191,255,255,128,0,43,28,85,85,85,85,113,128,57,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,85,213,213,128,170,255,213,57,85,0,170,255,128,0,128,255,255,128,128,0,170,255,255,255,255,255,255,170,64,0,170,255,170,85,85,85,85,57,0,0,142,255,213,128,85,85,85,28,85,64,43,191,255,255,255,255,255,85,128,43,0,43,85,128,85,85,85,28,43,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,57,0,0,0,85,227,170,85,85,170,170,0,43,64,128,255,191,64,0,64,85,0,0,128,64,213,255,255,255,191,85,0,0,43,0,57,128,170,213,255,227,85,85,0,85,142,43,0,85,213,255,128,128,0,128,255,191,128,191,255,213,64,64,0,43,85,85,85,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["ar", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,43,85,106,57,57,85,57,85,0,85,106,170,213,170,113,213,227,191,0,43,0,0,128,170,0,191,170,96,0,43,64,213,255,170,0,191,85,0,0,142,170,170,170,170,0,191,85,0,0,170,191,113,149,170,28,213,85,0,0,128,159,213,223,213,170,255,170,32,0,28,43,113,106,113,85,85,85,21,0,0,0,0,0,0,0,0,0,0,0]],
  ["F", [0,0,0,0,0,0,0,0,0,0,0,85,170,170,170,170,170,170,113,0,0,28,170,255,170,85,85,170,170,0,0,0,128,255,128,0,57,85,57,85,0,0,128,255,170,85,198,128,0,43,0,0,128,255,170,85,198,128,0,0,0,0,128,255,128,0,0,0,0,0,0,28,170,255,170,85,28,0,0,43,0,28,85,85,85,85,28,0,0,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["ru", [0,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,149,0,0,0,0,0,0,0,0,0,64,128,149,170,149,128,0,106,85,64,64,149,213,170,128,191,0,128,128,149,64,128,170,0,64,191,0,128,128,149,0,128,191,0,64,191,0,128,128,85,0,149,213,57,21,191,113,191,149,106,0,64,85,57,0,64,85,85,64,128,0,0,0,0,0,0,0,0,0,191,0,0,0,0,0,0,0,0,0,106,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,142,85,128,170,170,113,43,0,113,255,170,0,128,255,170,57,43,0,113,255,170,0,170,227,85,0,0,0,28,170,142,85,85,28,0,0,128,43,57,213,198,170,170,170,128,28,85,43,85,213,142,85,85,198,255,142,0,0,170,255,113,0,0,170,255,113,0,0,85,170,170,170,85,142,128,28,0,0,0,0,28,85,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0,85,85,170,170,170,170,170,128,28,0,43,57,85,85,85,170,255,255,85,0,0,28,85,128,170,213,255,255,85,0,0,170,255,128,0,128,255,255,85,0,0,142,255,213,128,213,255,255,113,43,85,28,85,85,85,85,85,85,57,43,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,43,85,85,85,85,43,0,0,0,0,43,170,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,43,170,255,255,255,170,85,43,0,0,43,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  // "ty" (not "ity" -- see appseg_notes.txt): capture A's "Frugality" has a
  // t/y anti-alias bridge whose valley ratio (~0.167) is numerically
  // indistinguishable from the "m"/"mm" internal saddle ratio (~0.17-0.18)
  // measured elsewhere in the same two captures -- no OCR_VALLEY_DEPTH_RATIO
  // value accepts one without also accepting the other, and accepting the
  // "m" case wrongly splits real single letters. Rather than risk that, the
  // "ty" bridge is left fused by segmentGlyphs and hand-harvested here as a
  // literal 2-letter blob, same technique already used elsewhere in this
  // atlas (see "th"/"an"/"ab" etc.) for shapes segmentation can't cleanly
  // separate. The "i" immediately before it in the same word DOES now
  // segment out on its own (see the "i" entry below, hand-harvested from
  // the same slot for the same reason: it's this word's *only* remaining
  // gap, verified by letter-position elimination, not guessed).
  ["ty", [64,21,0,0,0,0,0,0,0,0,191,64,21,43,0,0,0,0,0,0,64,21,106,128,0,0,0,0,0,0,170,85,213,213,170,170,64,106,106,0,213,85,170,170,85,255,85,128,85,0,191,64,128,128,0,198,128,149,64,0,191,64,128,128,0,85,213,128,0,0,213,106,128,213,113,28,213,128,0,0,85,64,43,85,57,0,149,85,0,0,0,0,0,0,0,85,149,21,0,0,0,0,0,0,0,170,106,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,43,85,85,85,85,43,0,0,0,0,128,255,255,255,255,128,0,0,0,0,43,85,85,85,85,43,0,0,85,170,170,170,170,170,170,85,0,0,43,85,170,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,43,85,170,255,255,255,255,170,85,0,43,85,85,85,85,85,85,85,85,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["H", [0,0,0,0,0,0,0,0,0,0,0,85,128,128,85,43,128,128,85,0,0,85,213,213,85,43,170,255,128,0,0,0,170,170,0,0,85,255,85,0,0,0,170,170,0,0,85,255,85,0,0,0,170,198,85,85,142,255,85,57,0,0,170,227,113,85,170,255,85,85,0,0,170,170,0,0,85,255,85,85,0,0,170,170,0,0,85,255,85,85,0,0,170,170,0,0,128,255,85,43,0,85,128,128,85,43,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,43,0,43,191,213,128,191,255,191,43,128,0,142,255,142,0,43,198,255,113,43,0,170,255,85,0,0,170,255,170,0,0,170,255,85,0,0,170,255,170,0,0,170,255,113,0,0,170,255,113,0,0,128,255,170,0,64,213,255,85,0,0,0,64,128,128,128,85,0,0,64,0,0,0,0,0,0,0,0,0,0]],
  ["m", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,85,64,85,85,106,113,21,0,43,191,213,223,255,255,255,255,96,43,113,149,227,106,198,198,85,198,128,113,170,128,170,0,170,170,0,170,128,170,170,128,170,0,170,170,0,170,128,170,113,128,170,0,170,170,0,170,128,142,85,128,213,32,170,170,0,170,128,85,0,96,128,96,128,128,96,128,96,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,85,28,43,0,43,191,255,191,191,255,255,128,128,0,113,255,213,85,43,170,255,170,43,0,170,255,213,170,170,213,255,170,0,0,170,255,255,191,128,128,128,85,0,0,142,255,255,128,0,0,0,0,0,0,85,255,255,191,128,128,128,85,0,64,0,64,128,128,191,128,128,43,64,0,0,0,0,0,43,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,43,43,85,113,43,0,64,128,255,255,191,191,255,255,128,0,128,43,142,255,255,213,170,170,85,0,128,0,85,255,255,128,0,0,0,0,64,0,85,255,255,128,0,0,0,0,0,0,85,255,255,128,0,0,0,0,64,0,85,255,255,128,0,0,0,0,0,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["Al", [0,0,0,0,0,0,0,21,21,0,0,0,0,57,43,0,28,170,106,0,0,0,32,213,159,0,0,191,128,0,0,0,64,255,191,0,0,191,128,0,0,0,106,142,191,43,0,191,128,64,0,0,128,85,191,64,0,191,128,159,0,0,159,43,159,128,0,191,128,191,0,21,170,85,170,149,0,191,128,191,0,64,170,85,128,191,0,191,128,191,0,96,128,0,64,223,43,191,128,159,0,106,106,0,64,170,142,170,106,64,0,0,0,0,0,0,28,21,0,0]],
  ["exan", [11,0,0,0,0,0,0,0,0,0,106,0,0,0,0,0,0,0,0,21,128,0,0,0,0,0,0,0,0,0,128,0,12,0,0,0,0,12,12,0,128,85,134,121,85,146,121,170,109,85,128,128,128,164,109,128,146,182,146,112,128,164,146,146,73,73,128,109,109,112,128,182,121,146,73,170,134,109,109,96,128,134,0,146,146,121,109,109,109,117,128,164,91,128,200,164,128,109,128,143,128,109,134,97,146,146,121,97,109,106,11,0,0,0,0,12,0,0,0,0]],
  ["de", [0,0,0,0,28,0,0,0,0,0,0,0,21,128,142,0,0,0,0,0,0,0,0,128,170,0,0,0,0,0,0,0,0,128,198,0,0,0,0,0,43,43,128,170,170,0,106,128,106,85,96,128,159,128,170,43,191,96,191,96,128,191,96,128,170,85,223,159,191,64,128,191,64,128,198,113,234,170,128,64,128,191,106,128,227,85,213,21,21,64,128,159,159,159,170,85,255,159,128,64,106,64,149,128,142,57,149,170,106,64,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,43,0,0,0,85,170,170,170,170,213,213,85,0,0,64,191,255,255,255,255,255,128,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,43,128,170,170,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["th", [0,0,0,0,21,43,0,0,0,0,0,0,0,0,128,170,0,0,0,0,0,0,64,0,96,191,0,0,0,0,0,43,170,0,64,191,0,21,0,0,0,149,234,170,106,234,170,191,64,64,0,159,223,128,96,223,128,223,128,128,0,64,191,0,64,191,0,128,128,191,0,64,191,0,64,191,0,128,128,191,0,64,191,0,64,191,0,128,128,191,0,64,223,128,96,191,0,128,128,159,0,43,170,170,106,149,57,128,106,64,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,28,0,0,0,0,0,0,0,0,0,28,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,170,128,128,170,170,57,0,0,85,255,255,128,64,191,255,128,0,0,170,255,191,128,128,191,255,170,0,0,170,255,213,170,170,170,170,113,0,0,170,255,213,85,0,0,0,0,0,0,128,255,255,191,128,128,128,85,0,43,28,128,170,170,213,170,170,85,0,0,0,0,0,0,43,0,0,0,0]],
  ["Gr", [0,0,0,0,0,0,0,0,0,0,0,0,32,128,128,32,0,0,0,0,0,32,223,128,159,64,0,0,0,0,0,106,191,21,85,85,106,85,85,21,0,159,128,0,0,64,223,223,191,96,0,191,128,21,85,43,149,213,128,170,0,191,128,43,213,128,128,128,0,191,0,159,128,0,128,64,128,128,0,191,0,128,170,0,128,64,128,128,0,191,0,96,223,32,128,64,128,128,0,159,0,0,64,128,128,64,128,128,32,32,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,28,85,128,170,128,57,0,0,128,0,128,255,191,128,191,213,64,0,85,85,227,170,43,0,128,255,128,0,0,128,255,213,170,170,213,255,128,0,0,128,255,191,128,128,128,128,64,0,0,128,255,170,43,0,0,28,43,85,0,64,213,255,191,128,128,170,128,64,0,0,43,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,128,170,113,43,0,43,64,43,191,255,255,255,255,191,43,128,128,28,85,85,85,85,198,255,85,43,128,28,85,113,170,170,227,255,85,0,64,85,255,213,128,128,213,255,85,0,43,142,255,170,0,0,170,255,85,0,128,128,255,213,128,128,213,255,85,0,0,43,128,128,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,191,255,128,0,0,0,0,0,43,170,255,255,170,85,85,43,0,0,128,255,255,255,255,255,255,128,0,0,43,170,255,255,170,85,85,43,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,191,128,128,64,0,64,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["M", [0,0,0,0,0,0,0,0,0,0,0,64,128,96,0,43,128,128,64,0,0,64,213,223,43,85,255,213,64,0,0,21,142,213,85,113,234,170,0,0,0,64,170,191,128,170,191,170,0,43,0,64,170,149,170,170,170,227,43,142,0,64,170,106,198,170,85,255,64,170,0,64,170,64,255,170,64,255,64,170,0,85,170,64,255,142,64,255,64,170,0,128,170,32,213,85,64,255,64,128,0,96,128,32,85,43,96,128,96,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,43,191,213,128,191,255,191,43,0,0,142,255,142,0,43,198,255,113,85,0,170,255,85,0,0,170,255,170,43,0,170,255,85,0,0,170,255,170,0,0,170,255,113,0,0,170,255,142,43,0,128,255,170,0,64,213,255,85,128,64,0,64,128,128,128,85,0,0,64,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,85,43,0,0,0,170,255,191,128,191,255,128,0,43,85,227,213,85,0,85,142,43,0,128,43,198,255,213,170,128,57,0,85,128,0,85,191,255,255,255,213,64,128,85,43,57,43,85,128,213,255,128,85,0,128,213,64,0,0,128,255,128,0,0,64,128,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,43,85,85,85,85,85,28,0,128,43,191,255,191,128,191,255,128,0,43,85,255,255,128,0,128,255,170,0,0,142,255,255,213,170,213,255,170,0,64,170,255,255,191,128,128,128,85,0,128,142,255,255,170,0,0,0,0,43,128,85,255,255,255,128,128,128,85,128,0,0,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,85,43,0,64,0,170,255,191,128,191,255,128,0,128,0,170,213,85,0,43,142,85,0,128,0,142,255,213,170,128,57,0,0,64,0,43,191,255,255,255,213,64,0,0,43,85,85,85,85,170,255,128,0,64,128,255,128,0,0,128,255,128,0,0,0,85,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["H", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,57,57,85,85,57,0,0,170,255,255,170,170,255,255,170,0,0,57,198,198,57,57,198,198,57,0,0,0,170,170,0,0,170,170,0,85,0,0,170,170,0,0,170,170,0,85,0,0,170,213,128,170,255,170,0,0,0,0,170,170,0,28,198,170,0,85,0,0,170,170,0,0,170,170,0,170,0,0,170,170,0,0,170,170,0,170,0,85,142,142,57,57,142,142,57,85,0,28,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,43,0,0,0,0,85,43,0,0,0,0,85,170,170,170,255,213,128,28,85,0,85,128,128,128,191,255,255,85,64,0,0,0,64,128,191,255,255,85,0,0,85,170,213,213,213,255,255,85,0,0,170,255,170,43,128,255,255,85,0,0,170,255,191,64,191,255,255,85,0,43,85,170,213,170,170,213,213,85,43,0,0,0,43,43,0,43,43,0,0]],
  ["m", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,0,0,0,21,28,0,0,28,128,170,149,170,170,191,198,64,113,85,159,255,159,213,213,128,213,128,128,85,128,170,0,170,170,0,170,128,0,85,128,170,0,170,170,0,170,128,0,85,128,170,0,170,170,0,170,128,0,85,128,213,32,170,170,0,170,128,43,85,106,170,85,142,142,64,142,106,85,0,0,0,0,0,0,0,0,0,0]],
  ["mu", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,17,17,17,34,0,0,0,0,0,85,153,170,170,187,85,153,85,102,102,153,179,179,230,179,102,204,102,153,128,153,102,102,179,102,102,204,102,153,102,153,102,102,153,102,102,204,102,153,102,153,102,102,153,102,102,187,102,153,102,153,128,102,179,102,128,153,128,153,102,119,119,102,153,102,119,102,136,102,85,0,0,0,0,0,0,0,17,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,128,128,170,170,85,43,0,64,191,255,255,255,255,255,128,64,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,43,0,0,128,255,255,0,0,0,0,128,0,0,128,255,255,0,0,0,0,128,0,43,128,170,170,85,43,0,0,43,0,0,0,0,0,0,0,0,0,0]],
  ["ab", [0,0,0,0,0,28,21,0,0,0,0,0,0,0,28,170,106,0,0,64,0,0,0,0,0,170,128,0,0,96,0,0,0,0,0,170,128,43,0,21,128,106,170,149,28,170,213,213,85,128,191,96,128,223,85,170,191,191,159,128,0,0,96,223,85,170,128,64,191,64,0,106,191,234,85,170,128,64,191,64,0,191,85,191,85,170,128,85,170,64,0,191,128,223,85,170,128,159,128,64,21,106,149,170,85,113,128,149,64,85,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,85,43,0,0,0,0,0,43,128,170,255,213,85,0,0,0,0,64,191,255,255,255,128,0,0,0,0,0,43,85,85,85,43,0,0,0,85,170,170,170,170,170,85,0,0,0,64,128,191,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,43,85,128,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["C", [0,0,0,0,0,0,0,0,0,0,0,0,0,28,85,85,85,85,57,0,0,0,0,128,255,191,213,255,170,0,0,28,128,198,128,43,113,255,170,0,0,142,255,142,0,0,28,128,113,43,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,142,0,0,0,0,0,0,0,142,255,198,43,0,28,128,113,0,0,43,191,255,191,0,85,255,170,0,0,0,43,142,170,85,113,170,113,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,43,128,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,85,85,170,113,0,85,142,170,57,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,85,85,255,170,0,0,170,255,85,0,128,85,255,213,64,64,213,255,85,0,85,28,128,170,128,85,142,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,0,28,28,0,0,0,0,0,0,0,85,198,85,0,0,0,0,0,0,0,85,255,85,0,0,85,43,0,0,0,85,255,85,0,85,227,85,0,0,57,85,255,85,43,213,255,198,113,85,85,85,255,85,0,191,255,170,85,85,85,85,255,85,0,128,255,85,0,85,85,85,255,85,0,128,255,85,0,85,85,85,255,85,0,128,255,85,0,85,85,85,255,85,0,128,255,170,85,85,85,85,170,113,43,85,170,170,113,28,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,85,170,113,0,128,170,85,0,85,64,85,255,170,0,128,255,128,0,64,0,85,255,170,0,128,255,128,0,0,0,85,255,170,0,128,255,128,0,0,0,85,255,170,0,128,255,128,0,0,64,85,255,213,64,191,255,128,0,0,85,28,128,170,128,128,142,128,57,43,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,170,128,170,170,85,0,0,64,191,255,255,255,255,255,128,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,43,0,0,128,255,255,0,0,0,0,128,0,0,128,255,255,0,0,0,0,128,43,43,128,170,170,85,43,0,0,43,0,0,0,0,0,0,0,0,0,0]],
  ["ra", [0,0,0,0,0,0,0,21,0,0,0,0,0,0,0,0,85,191,57,0,0,0,0,0,0,0,43,223,85,0,0,0,0,0,0,0,0,191,85,0,113,57,170,170,142,28,0,191,85,0,170,43,128,170,255,85,0,191,85,0,0,0,64,170,255,85,0,191,85,0,0,85,191,198,255,85,0,191,85,0,0,170,149,85,255,85,0,191,85,0,0,170,159,85,255,85,43,223,85,0,0,85,149,85,142,113,113,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["lH", [0,0,0,0,0,0,0,0,0,0,0,85,128,128,43,43,128,128,85,0,0,128,255,128,0,43,213,213,43,0,0,85,255,85,0,0,170,170,0,0,0,85,255,85,0,0,170,170,0,43,0,85,255,142,85,85,198,170,0,113,0,85,255,142,85,85,198,170,0,170,0,85,255,85,0,0,170,170,0,170,0,85,255,85,0,0,170,170,0,142,0,85,255,85,0,0,170,170,0,85,0,85,128,85,0,43,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,64,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,43,0,43,191,255,191,191,255,191,43,128,0,113,255,170,43,43,170,255,113,43,0,170,255,213,170,170,213,213,113,0,0,170,255,191,128,128,128,64,0,0,0,142,255,170,43,0,0,0,0,0,0,85,255,255,191,64,64,128,43,0,64,0,0,64,128,128,128,64,0,64,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,43,85,85,43,43,0,128,255,255,255,191,255,255,128,64,43,43,85,170,255,170,128,85,43,0,85,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,170,128,43,0,0,0,0,128,255,255,255,255,128,0,0,0,0,0,64,128,128,64,0,0,0,0,43,85,85,85,85,85,43,0,0,0,64,191,255,255,255,255,128,0,0,0,0,43,170,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,43,85,170,255,170,85,85,43,0,0,64,191,255,255,191,128,128,64,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,43,0,0,64,191,255,191,64,64,64,0,64,0,0,64,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,43,85,85,85,85,85,28,0,64,43,191,255,191,255,255,255,85,0,0,28,85,85,43,85,170,255,85,0,0,0,43,85,128,128,170,255,85,0,0,43,191,255,191,64,128,255,85,0,0,113,255,170,43,0,128,255,85,0,64,85,255,191,64,64,191,255,85,0,0,0,64,128,128,64,64,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,57,43,57,0,0,43,191,170,128,191,213,191,170,0,0,142,255,85,0,43,198,170,57,43,0,142,255,85,0,43,170,85,0,128,0,43,191,170,128,191,128,0,0,128,0,57,128,85,85,85,28,0,0,43,0,85,191,128,128,64,0,0,0,0,0,43,128,128,128,128,170,255,85,0,0,142,128,0,0,0,85,255,85,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,0,43,85,85,85,57,0,0,128,0,43,128,128,128,128,170,64,0,43,43,170,128,0,0,0,170,128,0,0,128,255,170,128,170,170,170,85,0,0,128,255,128,64,128,128,43,0,0,0,43,198,128,0,0,0,0,0,0,0,0,128,191,128,64,0,85,64,0,0,0,0,64,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["C", [0,0,0,0,0,85,85,43,0,0,0,0,0,85,128,128,170,191,85,0,0,0,85,198,128,0,28,170,170,0,0,43,191,170,0,0,0,128,170,0,0,85,255,170,0,0,0,43,57,0,0,128,255,170,0,0,0,0,0,0,0,128,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,28,0,0,43,191,213,64,0,0,64,128,0,0,0,85,227,213,85,113,213,170,0,0,0,0,85,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,64,0,0,0,57,170,170,128,85,170,213,85,0,0,128,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,113,255,213,128,128,213,255,142,0,0,43,128,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,43,128,170,170,170,85,0,0,0,0,0,64,191,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,43,170,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,43,170,255,255,255,170,85,43,0,0,64,128,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,128,128,64,0,0,0,28,128,142,85,85,198,213,57,0,0,128,255,85,0,0,128,255,128,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,128,255,128,0,0,128,255,128,0,43,57,213,198,85,85,170,170,57,0,64,0,64,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,128,128,64,0,0,85,213,213,128,85,170,255,128,0,0,128,255,191,64,0,64,128,64,0,0,64,191,255,255,191,128,64,0,0,0,0,43,85,128,213,255,213,85,0,0,64,128,64,0,64,191,255,128,0,0,128,255,170,85,128,213,213,85,0,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,128,128,128,85,0,64,0,85,227,170,85,85,170,170,0,43,0,128,255,128,0,0,64,85,0,0,0,64,213,255,255,191,128,43,0,0,0,0,57,128,170,213,255,170,43,0,0,64,85,0,0,64,191,255,128,0,0,128,198,85,85,128,213,170,43,0,0,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,85,0,128,128,64,0,0,0,113,255,170,0,170,255,128,0,85,0,85,255,170,0,128,255,128,0,128,0,85,255,170,0,128,255,128,0,64,43,85,255,170,0,128,255,170,28,0,128,85,255,170,0,128,255,255,85,64,43,57,213,227,128,170,227,255,113,128,0,0,64,170,191,64,128,191,85,64,0,0,0,28,43,0,28,43,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,128,128,128,85,0,0,0,85,227,170,85,85,170,170,0,0,0,128,255,128,0,0,64,85,0,0,0,64,213,255,255,255,191,43,0,0,0,0,57,128,170,213,255,170,43,0,0,64,85,0,0,64,191,255,128,0,43,128,227,128,85,128,213,227,85,0,64,64,128,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,85,85,128,142,43,0,0,85,227,128,0,0,85,227,128,0,0,128,255,213,170,170,170,227,128,0,0,128,255,128,0,0,0,0,0,0,0,43,170,213,170,170,170,142,43,0,0,0,28,85,85,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,113,170,142,113,128,170,128,28,0,0,113,255,198,57,43,198,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,43,113,255,198,28,43,198,255,113,0,43,57,85,85,28,43,85,85,57,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["R", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,43,0,0,0,0,170,255,191,128,213,191,85,0,0,0,113,255,128,0,113,255,170,0,0,0,85,255,128,0,85,255,170,0,85,0,85,255,128,0,85,255,170,0,64,0,85,255,255,255,255,191,43,0,0,0,85,255,170,85,198,213,57,0,0,0,85,255,128,0,113,255,170,28,0,0,85,255,128,0,43,191,255,85,0,0,85,170,128,57,0,85,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["H", [0,0,0,0,0,0,0,0,0,0,0,85,128,128,43,43,128,128,85,0,0,85,213,213,43,43,170,255,128,0,0,0,170,170,0,0,85,255,85,0,0,0,170,170,0,0,85,255,85,0,0,0,170,198,85,85,142,255,85,57,0,0,170,198,85,85,142,255,85,85,0,0,170,170,0,0,85,255,85,85,0,0,170,170,0,0,85,255,85,85,0,0,170,170,0,0,85,255,85,43,0,85,128,128,43,43,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,43,191,255,191,191,255,191,43,0,0,142,255,170,43,43,170,255,142,0,0,170,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,170,255,128,0,0,128,255,170,0,0,128,255,191,64,64,191,255,128,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["m", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,57,64,85,28,64,85,21,0,0,191,213,223,255,170,223,255,64,0,57,149,198,85,198,198,85,198,106,57,85,128,170,0,170,170,0,170,128,142,85,128,170,0,170,170,0,170,128,170,85,128,170,0,170,170,0,170,128,113,43,128,170,0,170,170,0,170,128,85,0,96,128,64,128,128,64,128,96,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,43,0,43,0,0,64,191,191,128,191,191,43,64,0,57,213,170,43,0,128,255,142,0,0,142,255,213,170,170,213,255,170,0,0,170,255,191,128,128,128,128,85,0,0,113,255,213,85,0,0,0,0,0,0,85,255,255,191,128,128,128,85,0,64,0,64,128,128,128,128,128,43,64,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,43,0,43,85,43,0,0,64,213,255,128,64,191,255,128,0,85,0,113,255,213,170,128,113,85,0,128,0,85,255,255,128,0,0,0,0,64,0,85,255,255,128,0,0,0,0,0,0,85,255,255,128,0,0,0,0,64,0,85,255,255,128,0,0,0,0,0,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["C", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,28,0,0,0,0,85,191,128,170,255,128,0,0,28,128,198,128,0,57,213,170,0,0,113,255,142,0,0,0,85,113,43,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,113,0,0,0,0,0,0,0,113,255,170,0,0,0,85,113,0,0,43,191,213,64,0,43,191,170,0,0,0,43,142,128,85,113,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,64,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,85,85,170,113,0,43,142,170,57,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,85,85,255,170,0,0,170,255,85,0,128,43,191,213,64,64,213,255,85,0,43,0,85,170,128,85,142,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,0,0,0,0,0,0,0,0,0,0,43,128,170,170,170,85,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,43,128,170,170,170,128,85,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,128,64,0,0,0,0,0,0,43,170,255,128,0,0,0,0,0,43,170,255,255,170,85,85,43,43,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,191,128,128,64,0,43,0,85,170,170,170,170,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,170,113,0,43,113,170,57,0,0,85,255,170,0,0,85,255,85,0,0,85,255,170,0,0,85,255,85,0,0,85,255,170,0,0,85,255,85,0,0,85,255,170,0,0,85,255,85,0,64,85,255,170,0,0,128,255,85,0,85,28,128,142,85,85,113,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,170,128,170,170,85,0,0,64,191,255,255,255,191,191,128,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,85,0,0,128,255,255,0,0,0,0,64,0,43,128,170,170,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,28,128,170,128,170,170,128,28,0,128,43,128,128,64,128,191,255,85,0,0,0,0,0,64,128,191,255,85,0,0,28,128,170,170,170,213,255,85,0,0,142,255,170,43,0,128,255,85,0,0,128,255,191,64,64,191,255,85,0,0,57,170,170,128,85,128,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["H", [0,0,0,0,0,0,0,0,0,0,0,85,128,128,43,43,128,128,85,0,0,43,213,128,0,0,170,213,43,0,0,0,170,85,0,0,170,170,0,0,0,0,170,85,0,0,128,170,0,0,0,0,170,142,85,85,142,170,0,57,0,0,170,142,85,85,142,170,0,142,0,0,170,85,0,0,85,170,0,128,0,0,170,85,0,0,85,170,0,85,0,43,213,85,0,0,85,170,0,43,0,85,128,128,43,43,128,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,64,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,43,0,43,0,0,64,191,191,128,191,191,43,64,0,57,213,170,43,0,128,255,113,0,0,142,255,170,128,170,170,170,113,0,0,128,255,128,64,128,64,0,0,0,0,85,255,170,43,0,0,0,0,0,0,43,191,255,191,0,0,64,43,0,0,0,0,64,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,0,43,85,43,0,0,64,191,255,255,64,191,255,128,0,43,0,43,170,255,85,85,85,43,0,85,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,43,85,170,255,170,85,85,43,0,0,64,191,255,255,191,128,128,64,0,0,0,43,170,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,64,0,0,0,64,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,28,0,0,0,64,213,191,128,191,255,128,0,0,0,43,85,43,0,43,170,170,0,0,0,0,57,85,128,128,170,170,0,0,0,64,213,191,128,64,128,170,0,0,0,128,198,43,0,0,128,170,0,0,0,128,213,64,0,0,128,170,0,0,0,0,85,128,64,0,64,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,57,43,57,0,0,43,191,170,128,128,170,191,170,0,0,142,255,85,0,0,170,170,57,0,0,142,255,85,0,43,170,85,0,85,0,43,191,170,128,191,128,0,0,64,0,57,170,113,85,85,28,0,0,0,0,85,191,85,0,0,0,0,0,0,0,43,128,128,128,128,170,255,85,0,0,142,128,0,0,0,85,255,113,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,0,43,85,85,85,57,0,0,128,0,43,128,128,128,128,170,64,0,43,0,142,128,0,0,0,170,128,0,0,85,227,213,170,170,170,170,85,0,0,64,213,191,128,128,128,43,0,0,0,0,170,128,0,0,0,0,0,0,0,0,128,191,128,64,0,43,64,0,0,0,0,64,128,128,128,43,0,0,43,0,0,0,0,0,0,0,0,0]],
  ["P", [0,0,0,0,0,0,0,0,0,0,0,85,128,128,128,128,85,0,0,0,0,85,191,255,191,64,170,255,128,0,0,0,128,255,128,0,85,255,170,0,0,0,128,255,128,0,85,255,170,0,0,0,128,255,128,0,85,255,142,0,0,0,128,255,170,128,170,170,57,0,0,0,128,255,191,128,85,0,0,0,0,0,128,255,128,0,0,0,0,85,0,0,128,255,128,0,0,0,0,64,0,85,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,0,0,0,0,0,0,0,0,0,128,0,43,85,85,85,85,43,0,43,128,43,191,255,191,255,255,191,43,64,85,28,85,85,43,85,170,255,85,0,0,0,43,85,128,170,213,255,85,0,0,43,191,255,191,128,191,255,85,0,0,142,255,170,43,0,128,255,85,0,0,128,255,191,64,64,191,255,85,0,0,43,128,128,128,64,64,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,64,191,128,0,0,0,0,0,43,85,170,255,170,85,85,43,0,0,64,191,255,255,255,191,128,64,0,0,0,43,170,255,170,43,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,0,0,128,255,191,64,64,64,0,64,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,43,43,85,85,43,43,0,128,255,255,128,191,255,255,128,128,0,43,170,255,213,128,85,128,85,43,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,64,128,128,128,128,64,0,0,64,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,85,43,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,64,128,128,128,64,0,0,0,0,43,85,85,85,85,43,0,0,0,0,128,255,255,255,255,128,0,0,0,0,43,128,213,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,64,128,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,57,0,0,43,0,43,128,170,128,128,170,64,0,64,0,113,255,113,0,0,142,213,57,0,0,170,255,85,0,0,85,255,142,0,0,170,255,85,0,0,85,255,170,0,0,142,255,85,0,0,113,255,113,0,0,85,255,128,0,0,170,191,43,0,64,0,64,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,43,85,170,255,170,85,85,43,0,0,64,191,255,255,191,128,128,64,0,0,0,128,255,255,128,0,0,0,0,85,0,128,255,255,128,0,0,0,0,128,0,128,255,255,128,0,0,0,0,43,0,128,255,255,128,0,0,0,0,0,0,128,255,255,191,128,128,64,0,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,85,85,43,0,0,0,64,191,255,255,255,255,128,0,0,0,0,64,128,128,64,0,0,0,0,0,43,85,85,85,85,85,43,0,0,0,128,255,255,255,255,255,128,0,0,0,43,170,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,64,128,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,64,191,191,128,128,191,128,0,0,0,128,255,170,43,0,85,85,0,0,0,85,213,255,213,128,85,43,0,0,0,0,64,128,191,255,255,191,64,0,0,43,43,0,43,170,255,255,128,0,0,128,191,64,0,128,255,255,128,0,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["m", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,43,57,85,57,85,64,0,0,0,170,191,170,213,213,213,223,43,0,0,113,213,57,113,170,57,213,85,0,0,85,191,0,85,170,0,149,85,0,43,85,191,0,85,170,0,128,85,0,85,85,191,0,85,170,0,128,85,0,85,85,191,0,85,128,0,128,85,0,0,85,128,43,85,128,43,96,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["C", [0,43,191,128,0,0,0,128,170,0,0,128,255,85,0,0,0,64,85,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,43,0,0,85,213,170,43,0,0,85,142,0,0,0,64,213,191,128,128,191,170,0,0,0,0,85,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,64,0,0,0,0,64,128,128,128,191,191,43,0,0,85,213,128,0,0,85,213,142,0,0,170,255,128,0,0,0,128,170,0,0,170,255,128,0,0,0,128,170,0,0,170,255,128,0,0,0,128,170,0,0,170,255,128,0,0,0,128,170,0,0,113,255,170,43,43,128,213,142,0,0,43,191,255,191,191,255,191,43,0,0,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,64,128,191,255,255,191,128,64,0,0,64,128,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,128,85,0,0,0,0,0,64,170,128,128,170,128,43,0,0,57,213,113,0,0,113,255,113,0,0,85,255,85,0,0,85,255,170,0,0,128,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,128,255,85,0,0,85,255,128,0,0,57,213,142,0,0,113,213,57,0,64,0,64,170,128,128,170,64,0,0,64,0,0,85,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,128,64,0,0,0,64,191,191,128,128,191,191,64,0,0,128,255,170,43,0,85,170,85,0,0,128,255,255,191,64,0,0,0,0,0,64,128,191,255,191,128,64,0,0,0,0,0,64,128,255,255,191,64,0,0,0,0,0,0,191,255,255,128,0,0,85,128,43,0,85,213,255,128,0,0,128,255,191,128,191,255,191,64,0,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,43,128,128,128,128,85,0,64,0,0,128,191,128,128,191,170,0,64,0,85,227,128,0,0,85,113,0,0,0,64,213,191,128,64,0,0,0,0,0,0,128,255,255,191,128,43,0,0,0,0,43,128,191,255,255,128,0,0,0,0,0,0,64,191,255,213,64,0,0,85,113,0,0,43,170,227,85,0,0,128,213,128,128,128,191,128,0,0,0,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,85,128,85,0,64,128,64,0,0,0,128,255,170,0,64,213,128,0,0,0,85,255,170,0,0,170,128,0,85,0,85,255,170,0,0,170,128,0,64,0,85,255,170,0,0,170,128,0,0,0,85,255,170,0,0,170,128,0,0,64,85,255,170,0,0,170,128,0,0,85,85,255,170,0,43,198,170,28,85,0,43,191,213,128,128,213,255,128,128,0,0,64,128,128,64,85,128,85,64,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,43,128,128,128,128,85,0,0,0,0,128,191,128,128,191,170,0,0,0,85,227,128,0,0,85,113,0,0,0,64,213,191,128,64,0,0,0,0,0,0,128,255,255,191,128,43,0,0,0,0,43,128,191,255,255,128,0,0,0,0,0,0,64,191,255,213,64,0,0,85,113,0,0,43,170,227,85,0,64,128,213,128,128,128,191,128,0,0,64,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["M", [0,0,0,0,0,0,0,0,0,0,0,64,128,96,0,0,96,128,64,0,0,64,213,255,85,85,255,213,64,0,0,0,113,213,85,85,234,170,0,0,0,32,128,159,128,128,159,213,32,43,0,64,170,128,170,170,128,255,64,142,0,64,170,106,198,170,106,255,64,170,0,64,170,64,255,170,64,255,64,170,0,64,170,43,227,142,64,255,85,170,0,96,170,0,170,85,64,255,128,128,0,96,128,64,85,43,64,128,96,0,0,0,0,0,0,0,0,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,85,85,85,43,0,0,0,43,191,213,128,191,255,191,43,0,0,142,255,142,0,43,198,255,113,85,0,170,255,85,0,0,113,255,170,43,0,170,255,85,0,0,85,255,170,0,0,170,255,113,0,0,142,255,142,43,0,128,255,170,0,64,213,255,85,128,64,0,64,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,85,85,85,85,85,43,0,0,0,128,255,191,128,191,255,128,0,43,85,227,213,85,0,43,142,85,0,128,43,170,255,213,170,128,57,0,43,128,0,43,191,255,255,255,213,64,128,85,43,85,85,85,128,213,255,128,43,0,128,255,128,0,0,128,255,128,0,0,0,85,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,0,43,85,85,85,85,28,0,128,0,64,191,255,128,191,255,128,0,85,57,213,255,170,0,128,255,170,0,0,113,255,255,213,170,213,255,170,0,64,170,255,255,191,128,128,128,85,0,128,113,255,255,170,0,0,0,28,0,128,85,255,255,255,128,128,128,128,0,0,0,64,128,128,128,128,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["s", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,57,0,0,0,64,213,191,128,128,191,170,0,0,0,128,255,170,43,0,85,113,0,0,0,85,227,255,213,170,128,28,0,0,0,0,85,191,255,255,255,128,0,0,0,43,57,43,85,170,255,227,85,0,0,128,170,0,0,128,255,213,64,0,0,64,128,128,128,128,128,85,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,28,128,85,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,43,128,85,0,0,0,0,0,0,0,57,85,57,0,85,85,85,28,43,0,142,255,170,0,85,198,255,85,43,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,64,213,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,113,43,64,128,255,213,128,0,170,255,213,191,43,57,85,85,85,0,57,85,85,85,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,43,85,85,85,43,0,0,0,128,0,43,170,255,255,128,0,0,85,128,0,0,128,255,255,128,0,0,128,128,0,0,128,255,255,128,0,0,64,128,0,0,128,255,255,128,0,0,0,170,43,43,170,255,255,128,0,0,85,255,191,191,191,191,255,191,128,64,128,85,85,85,43,85,170,128,85,43,43,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,85,85,85,128,85,85,28,43,0,85,170,170,170,213,255,255,85,85,0,0,0,0,0,128,255,255,85,0,0,43,128,191,255,255,255,255,85,0,85,142,255,213,128,170,255,255,85,0,128,170,255,170,43,170,255,255,85,0,64,128,255,255,191,255,255,255,170,128,0,28,85,128,128,85,128,128,85,85,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,28,85,85,128,43,85,128,85,0,128,57,170,213,255,213,255,255,170,0,128,0,0,128,255,191,128,128,85,0,128,0,0,128,255,128,0,0,0,0,128,0,0,128,255,128,0,0,0,0,128,0,43,170,255,128,0,0,0,0,191,128,191,255,255,191,128,64,0,0,85,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,57,170,170,170,170,170,128,28,0,128,43,128,128,128,191,255,255,85,0,0,0,0,64,128,191,255,255,85,0,0,85,170,213,213,213,255,255,85,0,0,170,255,170,43,128,255,255,85,0,0,170,255,191,64,128,255,255,85,0,0,85,170,170,128,85,128,170,113,85,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,43,43,0,0,0,0,0,43,128,170,213,213,85,0,0,0,0,0,64,191,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,43,0,0,0,128,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,128,0,0,64,191,255,255,128,0,0,128,85,85,128,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,128,64,0,0,64,0,64,191,255,255,64,0,0,0,64,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,85,85,85,85,85,0,0,128,255,255,85,85,85,85,85,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,64,128,128,128,64,0,0,0,64,0,0,0,0,0,0,0,0,0,0]],
  ["H", [0,0,0,0,0,0,0,0,0,0,0,0,64,128,128,128,128,128,64,0,0,0,64,191,255,255,191,64,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,85,85,85,170,255,255,128,0,0,43,85,85,85,170,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,85,0,0,0,128,255,255,128,0,0,0,0,0,64,128,128,128,128,128,64,0,0,0,0,0,0,0,0,0,0,0]],
  ["L", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,43,0,0,0,0,0,128,255,255,191,64,0,0,0,0,0,57,213,198,43,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,28,0,0,85,255,170,0,0,0,85,142,0,0,85,255,170,0,0,0,128,170,0,0,85,170,142,85,85,85,128,113,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,128,85,43,0,0,0,0,128,255,255,191,128,64,0,0,0,0,43,85,85,43,0,0,0,0,0,85,170,170,170,170,170,85,0,0,0,64,191,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,43,128,170,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["b", [0,0,0,0,0,0,0,0,0,0,0,85,170,113,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,170,0,0,0,0,0,0,0,85,255,227,128,128,170,128,28,0,0,85,255,213,64,64,170,255,128,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,142,0,0,85,255,170,0,0,128,255,85,0,43,57,170,142,85,85,113,85,28,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,128,128,170,170,85,0,0,64,191,255,191,191,128,191,128,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,43,0,0,128,255,255,0,0,0,0,128,0,0,128,255,255,0,0,0,0,128,0,43,128,170,170,85,43,0,0,43,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,28,128,128,85,128,170,128,28,0,128,43,128,64,0,64,191,255,85,0,0,0,0,0,64,128,191,255,85,0,0,85,170,170,170,170,213,255,85,0,0,170,255,170,43,0,128,255,85,0,0,170,255,191,64,64,191,255,85,0,0,85,170,170,128,85,128,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,85,128,170,170,128,128,0,64,191,255,191,255,191,191,128,128,0,0,128,255,255,0,0,0,0,64,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,0,128,255,255,0,0,0,0,0,0,43,128,170,170,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["y", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,170,142,170,142,43,0,85,170,113,0,191,170,255,170,0,0,85,255,128,0,0,43,191,213,64,0,85,255,85,0,0,0,85,227,128,0,142,170,28,0,0,0,0,170,213,128,170,85,0,0,0,0,0,128,255,255,170,0,0,0,0,0,0,57,213,255,142,0,0,0,0,0,0,28,170,213,57,0,0,0]],
  ["o", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,113,85,85,142,128,28,85,0,43,191,128,0,0,170,255,85,64,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,142,255,113,0,0,113,255,142,0,0,85,255,170,0,0,170,255,85,0,0,28,128,142,85,85,113,85,28,43,0,0,0,0,0,0,0,0,0,0]],
  ["f", [0,0,0,0,0,0,43,85,43,0,0,0,0,0,43,170,213,255,128,0,0,0,0,64,191,191,128,128,64,0,0,0,0,128,255,128,0,0,0,0,0,85,170,213,255,213,170,85,0,0,0,64,128,191,255,191,128,64,0,0,128,0,0,128,255,128,0,0,0,0,128,0,0,128,255,128,0,0,0,0,85,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,0,0,43,85,128,170,128,85,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["A", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,28,0,0,28,0,0,0,0,128,255,128,0,0,0,0,0,0,28,170,213,170,0,0,0,0,0,0,85,213,128,227,57,0,0,0,0,0,128,128,64,213,85,0,0,0,0,0,128,64,0,170,170,0,0,0,0,57,170,85,85,198,198,28,0,0,0,142,170,85,85,142,255,85,0,0,0,170,85,0,0,43,213,128,0,0,57,142,113,43,0,57,142,142,85,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,0,0,0,0,0,0,0,0,0,0,0,0,43,128,170,170,85,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,43,0,0,0,128,255,255,128,0,0,128,0,0,0,128,255,255,128,0,0,191,64,0,0,128,255,255,128,0,0,170,128,85,85,128,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["e", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,128,85,85,128,113,0,43,0,64,213,128,0,0,128,213,64,0,0,128,213,128,128,128,191,255,128,0,0,128,227,170,170,170,170,170,85,0,0,128,227,85,0,0,0,0,0,0,0,64,213,191,128,128,128,128,64,0,0,0,85,170,170,170,170,142,43,43,0,0,0,0,0,0,0,0,0,0]],
  ["x", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,142,85,43,142,128,57,43,64,43,191,213,64,0,128,64,0,64,128,0,0,170,255,191,128,0,0,0,85,0,0,113,255,213,57,0,0,43,0,0,43,142,128,255,170,43,0,128,64,43,191,128,0,191,255,191,43,128,43,85,170,113,43,128,170,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,85,170,128,85,170,170,85,0,85,0,85,128,64,0,191,255,191,43,64,0,0,0,64,128,191,255,255,85,0,0,85,170,170,170,213,255,255,85,0,0,170,213,85,0,128,255,255,85,0,0,170,255,128,0,191,255,255,85,0,43,85,170,128,85,128,170,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,113,170,142,128,128,142,170,85,0,0,128,255,213,128,64,128,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,0,0,85,255,170,64,43,85,170,142,43,0,85,170,142,85,0,0,0,0,0,0,0,0,0,0]],
  ["d", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,43,113,170,57,0,0,0,0,0,0,0,85,255,85,0,0,0,0,0,0,0,85,255,85,0,142,28,0,85,170,85,142,255,85,113,255,85,43,213,191,0,85,255,85,85,255,85,85,213,64,0,85,255,85,43,255,85,85,170,0,0,85,255,85,0,255,85,85,227,85,0,85,255,85,0,255,128,170,255,191,0,128,255,85,43,170,113,85,142,170,85,113,170,85,85,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,170,128,170,170,85,43,0,64,128,191,255,255,191,128,64,0,0,0,64,191,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,0,64,191,255,0,0,0,0,0,43,43,128,170,170,85,85,43,0,43,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,0,43,128,170,170,170,85,0,0,0,0,64,191,255,255,191,64,0,0,0,0,0,43,85,85,43,0,0,0,0,43,128,170,170,170,170,85,0,0,0,0,64,191,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,0,0,128,255,255,255,128,0,0,0,43,85,128,170,170,170,128,43,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,170,128,170,170,128,28,0,0,43,128,128,64,128,191,255,85,0,0,0,0,0,64,128,191,255,85,0,0,57,170,170,170,170,213,255,85,0,0,142,255,170,43,0,128,255,85,0,0,128,255,191,64,64,191,255,85,0,43,57,170,170,128,85,128,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["F", [0,0,0,0,0,0,0,0,0,0,0,113,170,142,85,85,113,170,113,0,0,113,255,170,0,0,28,170,170,0,0,85,255,170,0,0,28,85,57,43,0,85,255,198,85,85,170,128,0,0,0,85,255,198,85,85,198,128,0,0,0,85,255,170,0,0,0,0,0,0,0,113,255,198,85,85,28,0,0,43,0,57,85,85,85,85,28,0,0,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,85,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,43,43,128,170,170,85,128,170,85,43,0,0,43,170,255,170,128,85,43,0,0,0,0,128,255,0,0,0,0,0,0,0,0,128,255,0,0,0,0,0,0,43,85,170,255,85,85,43,0,0,0,43,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["u", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,85,170,113,0,43,142,170,57,0,43,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,57,213,170,0,0,170,255,85,0,0,0,128,227,128,85,198,255,113,0,0,0,43,85,85,43,57,85,57,43,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0]],
  ["g", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,128,142,85,128,170,170,113,43,0,85,255,113,0,128,255,170,57,43,0,85,255,113,0,128,227,85,0,0,0,28,170,142,85,85,28,0,0,128,43,57,213,198,170,170,142,85,28,85,43,85,213,142,85,85,170,255,85,0,0,170,255,85,0,0,113,213,57,0,0,85,170,113,85,85,113,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0,85,85,170,128,85,170,170,85,0,0,43,57,85,43,0,170,255,213,57,0,0,28,85,85,128,213,255,255,85,0,0,170,255,128,0,128,255,255,85,0,0,142,255,170,85,213,255,255,113,43,0,28,85,85,85,85,85,85,57,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["l", [0,43,85,85,85,85,85,43,0,0,0,43,170,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,43,170,255,255,255,255,170,43,0,0,43,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,43,85,85,85,85,43,0,0,0,0,128,255,255,255,255,128,0,0,0,0,43,85,85,85,85,43,0,0,0,85,170,170,170,170,170,85,0,0,0,43,170,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,43,170,255,255,255,255,170,43,0,0,43,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["t", [0,0,0,0,0,0,0,0,0,0,0,0,0,43,43,0,0,0,0,0,0,0,43,170,128,0,0,0,0,0,0,43,170,255,213,128,85,128,128,128,0,0,128,255,255,128,0,43,43,85,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,128,0,0,0,0,0,0,128,255,255,213,170,128,43,0,0,0,43,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,0]],
  ["y", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,128,142,170,142,28,43,142,170,85,0,43,85,213,170,0,0,170,170,28,0,0,0,128,227,57,43,170,85,0,0,0,0,0,170,142,128,142,0,0,0,128,28,0,113,227,213,85,0,0,0,43,0,0,28,170,170,28,0,0,0,0,28,85,113,170,43,0,0,0,0,0,28,128,170,85,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["R", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,85,85,28,0,0,0,170,255,255,170,213,255,170,43,0,0,57,198,255,85,57,170,255,85,0,0,0,170,255,85,0,128,255,85,0,0,0,170,255,85,43,191,255,85,0,0,0,170,255,255,255,255,128,0,0,0,0,170,255,142,142,255,170,28,0,0,0,170,255,85,28,170,255,113,0,0,0,170,255,85,0,128,255,170,0,0,57,142,170,113,28,43,142,142,85,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,85,0,0,64,0,0,0,128,255,255,128,0,0,128,0,0,0,43,85,85,43,0,0,128,0,0,85,170,170,170,85,0,0,128,0,0,64,191,255,255,128,0,0,0,0,0,0,128,255,255,128,0,64,43,0,0,0,128,255,255,128,0,128,170,43,0,0,128,255,255,128,0,43,255,128,0,0,128,255,255,128,0,0,170,128,85,85,128,170,170,128,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["c", [0,0,0,0,0,0,0,0,0,0,85,0,0,0,0,0,0,0,28,85,128,0,0,0,0,0,0,0,0,0,43,0,0,0,0,0,0,0,0,0,85,0,0,85,170,128,113,170,113,0,128,0,64,213,191,64,85,255,128,0,128,43,191,255,128,0,43,128,43,0,128,85,255,255,128,0,0,0,0,0,128,28,170,255,170,43,0,0,0,0,128,0,128,255,255,191,128,128,85,0,128,57,43,142,170,170,170,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["h", [0,0,28,85,28,0,0,0,0,0,0,57,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,170,57,170,255,170,170,170,142,28,0,191,43,170,255,170,128,191,255,85,0,64,0,170,255,85,0,128,255,85,0,0,0,170,255,85,0,128,255,85,0,0,0,170,255,85,0,128,255,85,0,128,43,170,255,85,0,128,255,85,0,128,57,142,170,85,28,128,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["L", [0,0,0,0,0,0,0,0,0,0,0,57,85,85,85,43,0,0,0,0,0,170,255,255,255,128,0,0,0,0,0,57,170,255,170,43,0,0,0,0,0,0,128,255,128,0,0,0,0,43,0,0,128,255,128,0,0,0,0,64,0,0,128,255,128,0,0,0,0,0,0,0,128,255,128,0,0,43,57,43,0,0,128,255,128,0,57,213,170,128,0,43,191,255,128,0,85,255,128,128,0,85,170,170,128,85,113,170,57,43,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,170,170,170,170,170,128,28,85,0,85,128,128,128,191,255,255,85,64,0,0,0,64,128,191,255,255,85,0,43,85,170,170,170,213,255,255,85,0,128,170,255,128,0,128,255,255,85,0,64,170,255,191,64,191,255,255,85,0,0,85,170,170,128,170,170,170,85,43,0,0,0,0,0,0,0,0,0,0]],
  ["n", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,113,170,142,128,170,170,128,28,0,0,128,255,213,128,128,213,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,0,85,255,170,0,0,170,255,85,0,43,85,170,142,43,43,142,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["d", [0,0,0,0,0,0,0,43,28,0,0,0,0,0,43,85,142,213,85,0,0,0,0,0,0,0,170,255,85,0,0,0,0,0,0,0,170,255,85,0,0,28,128,142,85,85,198,255,85,0,0,128,255,128,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,85,0,0,170,255,128,0,64,213,255,85,0,0,85,170,142,85,128,170,170,85,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,43,85,170,128,85,43,0,0,0,0,128,255,255,255,255,128,0,0,0,0,64,128,128,128,128,64,0,0,43,85,85,85,85,85,85,43,0,0,85,170,213,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,0,0,128,255,255,255,255,128,0,0,64,128,191,255,255,255,255,191,128,0,43,85,85,85,85,85,85,85,85,0,0,0,0,0,0,0,0,0,0]],
  ["u", [113,28,0,0,0,0,0,0,0,0,255,85,0,0,0,0,0,0,0,0,128,43,0,0,0,0,0,0,0,0,85,28,57,85,57,28,85,57,0,28,255,85,57,198,170,28,198,170,0,142,255,85,0,170,170,0,170,170,0,170,255,85,43,213,170,0,170,170,0,128,255,85,0,170,170,0,170,170,0,28,255,85,0,170,198,57,198,170,0,113,255,170,43,170,255,213,213,213,85,170,85,85,28,57,85,85,85,142,57,57,0,0,0,0,0,0,0,0,0,0]],
  ["a", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,57,85,85,128,128,85,43,0,0,43,142,170,170,170,213,255,128,0,0,0,0,0,0,0,128,255,128,0,0,0,85,191,255,255,255,255,128,0,85,85,227,213,128,85,170,255,128,0,128,128,255,170,43,43,170,255,128,0,64,64,213,255,191,191,255,255,191,128,0,0,57,128,128,85,85,113,85,85,0,0,0,0,0,0,0,0,0,0]],
  ["r", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,85,28,43,85,128,85,57,128,85,0,255,85,85,198,255,213,227,255,170,0,255,85,0,85,255,255,170,128,85,0,255,85,0,85,255,255,85,0,0,0,255,85,0,85,255,255,85,0,0,0,255,85,0,113,255,255,85,0,0,0,255,170,128,213,255,255,170,64,0,0,128,85,85,85,85,85,85,43,0,0,0,0,0,0,0,0,0,0,0,0]],
  ["i", [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,170,170,170,85,0,0,0,0,0,0,255,255,255,128,0,0,0,0,0,0,85,85,85,43,0,0,0,0,85,170,170,170,170,85,0,0,0,0,64,128,255,255,255,128,0,0,0,0,0,0,255,255,255,128,0,0,0,0,0,0,255,255,255,128,0,85,43,0,0,0,255,255,255,128,0,255,128,0,0,0,255,255,255,128,0,170,128,85,85,85,170,170,170,128,85,0,0,0,0,0,0,0,0,0,0]],
  ["c", [0,0,0,0,0,0,0,0,0,0,170,57,0,0,0,0,0,0,28,85,255,85,0,0,0,0,0,0,0,0,85,28,0,0,0,0,0,0,0,0,170,57,0,28,128,128,113,170,113,0,255,85,0,128,255,64,85,255,128,0,255,85,64,213,255,0,43,128,43,0,255,85,128,255,255,0,0,0,0,0,255,85,43,198,255,43,0,0,0,0,255,85,0,170,255,191,128,128,85,0,170,113,43,85,170,170,170,170,85,43,0,0,0,0,0,0,0,0,0,0]],
];

// Distance beyond which a glyph is unrecognised rather than force-fit to
// the nearest atlas entry — chosen on the real capture (see notes.txt):
// true same-letter matches from this atlas land under ~35, cross-letter
// confusions start above ~55, so 45 sits in the gap.
const OCR_WILDCARD_DIST = 45;

function classifyGlyph(grid) {
  let best = null, bestDist = Infinity;
  for (const [label, atlasGrid] of OCR_ATLAS) {
    const d = gridDistance(grid, atlasGrid);
    if (d < bestDist) { bestDist = d; best = label; }
  }
  if (best === null || bestDist > OCR_WILDCARD_DIST) return '?';
  return best;
}

// Read every line's every glyph in a slot and concatenate labels in
// reading order. No word-space tracking (see segmentGlyphs) — the raw
// string is deliberately just letters-and-wildcards run together.
//
// The fusion-split median is computed once across every line in the slot,
// not per line: a two-line name can leave one line with only a couple of
// zero-ink boxes (e.g. a heavily-fused "Gardens" split into just 2 boxes
// by the coarse cut), and a median of 2 samples is easily dragged to ~2x
// a real letter width by a single still-fused box — which then raises the
// "stop splitting" bar enough to leave that very box under-split. Pooling
// both lines' boxes gives a far sturdier estimate at negligible cost.
function ocrSlotRaw(imageData, slot) {
  const lines = findTextLines(imageData, slot);
  const perLine = lines.map((line) => lineColInk(imageData, slot, line));
  const allWidths = perLine.flatMap(({ colInk }) => zeroInkBoxes(colInk).map((b) => b.end - b.start + 1));
  const slotMedian = median(allWidths) || undefined; // undefined -> segmentGlyphs falls back per-line
  let out = '';
  for (const line of lines) {
    const boxes = segmentGlyphs(imageData, slot, line, slotMedian);
    for (const box of boxes) out += classifyGlyph(glyphToGrid(imageData, slot, line, box));
  }
  return out;
}

// Case/punctuation/space-insensitive normalisation shared by the OCR
// string and every card name it's compared against — this is also why
// segmentGlyphs never has to work out which gaps were word-spaces.
function ocrNormalize(s) {
  return (s || '').toLowerCase().replace(/[^a-z0-9]/g, '');
}

function levenshtein(a, b) {
  const m = a.length, n = b.length;
  if (m === 0) return n;
  if (n === 0) return m;
  let prev = new Array(n + 1);
  let cur = new Array(n + 1);
  for (let j = 0; j <= n; j++) prev[j] = j;
  for (let i = 1; i <= m; i++) {
    cur[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost);
    }
    [prev, cur] = [cur, prev];
  }
  return prev[n];
}

// The answer space is closed (236 known names) — OCR never has to be
// perfect, it has to be UNAMBIGUOUS. Accept the best Levenshtein match
// only if it's close enough in absolute terms AND clearly better than
// the runner-up; otherwise UNKNOWN. A confident wrong guess silently
// corrupts the board the advisor reasons over, so the failure mode on
// any doubt is "ask the human", never "guess". Bounds chosen on the real
// capture (see notes.txt): every one of the 13 correct matches lands at
// distance <=2 with a runner-up gap >=3 (once compared name-to-name —
// see below), except EXACT (distance-0) matches, which skip the margin
// check entirely: if the OCR string is literally equal to a known name,
// there is no realistic ambiguity left to guard against.
const OCR_MATCH_DIST_BOUND = 4;
const OCR_MATCH_MARGIN = 3;

// Shared name-matching core: normalizes the OCR string and every card's
// base NAME (never the age-suffixed display — see resolveOCRString below
// for why) and picks the best Levenshtein match. Returns { group } where
// group is every card sharing that normalized name (1 entry when the name
// is unique in the pool, 2+ for an age-ambiguous family like "Rich Land"),
// or null when OCR didn't confidently match ANY name at all. Split out
// from resolveOCRString so the "name known, age unknown" UI hint (see
// ocrRecognizeSlot) can reuse the exact same match instead of re-deriving
// it with a second, potentially-diverging pass.
function matchOCRName(raw, cardList) {
  const query = ocrNormalize(raw);
  if (!query) return null;
  const byName = new Map(); // normalized base name -> [cards with that name]
  for (const c of cardList) {
    const n = ocrNormalize(c.name || c.display);
    if (!byName.has(n)) byName.set(n, []);
    byName.get(n).push(c);
  }
  let bestName = null, bestDist = Infinity, secondDist = Infinity;
  for (const [name] of byName) {
    const d = levenshtein(query, name);
    if (d < bestDist) { secondDist = bestDist; bestDist = d; bestName = name; }
    else if (d < secondDist) { secondDist = d; }
  }
  if (bestName === null) return null;
  if (bestDist !== 0) {
    if (bestDist > OCR_MATCH_DIST_BOUND) return null;
    if (secondDist - bestDist < OCR_MATCH_MARGIN) return null;
  }
  return { group: byName.get(bestName) };
}

// Compare against the card's base NAME, never its age-disambiguated
// display ("Rich Land (A)" vs "(I)" vs "(II)"). OCR reads only the
// printed name text — it never sees the age badge — so matching against
// the suffixed display would manufacture a spurious near-tie between a
// card and its own other-age copies, which is not real ambiguity, just
// an artifact of what string we happened to compare against.
//
// badgeAge (optional, from readAgeBadge — see below) settles a name that
// maps to more than one age copy: if exactly one member of the family
// carries that age, that's the card. Omitted, or the badge unreadable, or
// the badge age not present in the family (a misread) — stays an "ask a
// human" case exactly as before, never a guess.
function resolveOCRString(raw, cardList, badgeAge) {
  const match = matchOCRName(raw, cardList);
  if (!match) return null;
  const group = match.group;
  if (group.length === 1) return group[0];
  if (badgeAge) {
    const filtered = group.filter((c) => c.age === badgeAge);
    if (filtered.length === 1) return filtered[0];
  }
  return null;
}

/* ---------------------------------------------------------------------
 * Age badge — every printed card carries a small age medallion (A / I /
 * II / III) centered just above the name band (the same top strip the
 * name-OCR search deliberately excludes — see OCR_LINE_Y_LO above). This
 * is the piece resolveOCRString needs to settle a name that 2-4 age
 * copies share (Rich Land, Engineering Genius, Frugality, Cultural
 * Heritage in this capture's own pool).
 *
 * Region: calibrated against all 13 cards in the one real capture. The
 * medallion's own ink (the letter, not its gold/purple/green shield
 * background, which varies by card frame colour) lands within x
 * [0.452,0.538] / y [0.050,0.115] of slot size on every one of the 13 —
 * the box below pads that for margin against a different capture
 * resolution or font hinting, same "fraction, never absolute pixels"
 * rule as everything else in this file.
 *
 * Classifier: 4-class (A / I / II / III), so simpler than general glyph
 * OCR — and I/II/III differ from each other, and from A, purely by how
 * many separate vertical ink strokes a horizontal scan crosses. I/II/III
 * are 1/2/3 parallel bars running the FULL glyph height with no row-to-
 * row variation. 'A' is the only non-bar shape in this alphabet: a
 * scanline through its point or crossbar sees 1 run, through its open
 * legs sees 2 — so unlike a numeral, its run-count genuinely varies down
 * the glyph. That row-to-row (in)consistency, not the run-count value by
 * itself, is what tells 'A' apart from 'II' (both can show "2" on any
 * single row picked in isolation). Counting strokes this way was chosen
 * over template matching because there is no real captured I/II/III
 * badge to build templates from — every one of the 13 cards in the only
 * capture available reads 'A' (see appbadge_notes.txt) — while stroke-
 * counting only needs to know what a bar LOOKS like, not what one from
 * this exact font/resolution looks like.
 * ------------------------------------------------------------------- */
const AGE_BADGE_X_LO = 0.40, AGE_BADGE_X_HI = 0.60;
const AGE_BADGE_Y_LO = 0.035, AGE_BADGE_Y_HI = 0.135;
const AGE_BADGE_MIN_INK = 8; // fewer lit pixels than this in the whole box -> nothing readable there
const AGE_BADGE_RUN_CONSISTENCY = 0.8; // fraction of ink rows that must agree on one run-count to call it a bar numeral

// Badge letter is white/pale on every card frame colour tested (gold,
// purple, green) — the opposite polarity from isInk() (which finds DARK
// desaturated name text further down the same card): bright AND low
// saturation, so it doesn't false-positive on the (saturated) frame art
// immediately around the medallion. Threshold tuned looser than a first
// pass (luma>195) because the letter's own anti-aliased fill dims well
// below that on the green/purple leader-card frames (down to ~luma 170)
// even though it hits ~250 on the gold action-card frames — a single
// luma>195 cutoff read as a broken, mostly-1-run scribble on green/purple
// cards (visually still a clean 'A', just fainter) and would have voted
// those into a false 'I'. luma>165/sat<50 recovers a clean, fully-
// connected 'A' shape on every one of the 13 cards in this capture
// (checked directly against the ascii dumps used to tune this).
function isBadgeInk(r, g, b) {
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b);
  const luma = 0.299 * r + 0.587 * g + 0.114 * b;
  return luma > 165 && (mx - mn) < 50;
}

// Returns 'A' | 'I' | 'II' | 'III' | null. null = unreadable — no ink, or
// ink present but neither a consistent bar-count nor enough of it to call
// 'A' either (e.g. this region of a different, badge-less image) — the
// caller must treat null exactly like OCR's own '?': never guessed.
function readAgeBadge(imageData, slot) {
  const { data, width, height } = imageData;
  const x0 = slot.x * width, y0 = slot.y * height;
  const w = slot.w * width, h = slot.h * height;
  const bx0 = Math.round(w * AGE_BADGE_X_LO), bx1 = Math.round(w * AGE_BADGE_X_HI);
  const by0 = Math.round(h * AGE_BADGE_Y_LO), by1 = Math.round(h * AGE_BADGE_Y_HI);
  function px(x, y) {
    const i = (Math.round(y0 + y) * width + Math.round(x0 + x)) * 4;
    return [data[i], data[i + 1], data[i + 2]];
  }
  let totalInk = 0, countedRows = 0;
  const counts = {}; // run-count (strokes crossed in that row) -> how many rows had that count
  for (let y = by0; y < by1; y++) {
    let runs = 0, inRun = false, rowInk = 0;
    for (let x = bx0; x < bx1; x++) {
      const [r, g, b] = px(x, y);
      if (isBadgeInk(r, g, b)) {
        rowInk++; totalInk++;
        if (!inRun) { runs++; inRun = true; }
      } else {
        inRun = false;
      }
    }
    if (rowInk > 0) { counts[runs] = (counts[runs] || 0) + 1; countedRows++; }
  }
  if (totalInk < AGE_BADGE_MIN_INK || countedRows === 0) return null;
  let bestRuns = 0, bestFreq = 0;
  for (const k in counts) {
    if (counts[k] > bestFreq) { bestFreq = counts[k]; bestRuns = +k; }
  }
  const consistency = bestFreq / countedRows;
  if (consistency >= AGE_BADGE_RUN_CONSISTENCY && bestRuns >= 1 && bestRuns <= 3) {
    return ['I', 'II', 'III'][bestRuns - 1];
  }
  return 'A'; // real ink present, but not a consistent bar count -> the one non-bar glyph
}

// Full per-slot entry point: raw OCR string + resolved card (or null) +
// the age badge read for this slot + a nameHint for the UI (Job 2) when
// the name matched a family but no single card could be settled (age
// unreadable, or read but not unique/absent from the family) — nameHint
// is null whenever resolved is non-null (nothing left to hint at) and
// also null when OCR didn't match any name at all (there's no better
// starting point to offer than a blank input in that case).
// cardList defaults to the module-level CARDS (populated by loadCards()
// before any scan happens in the real app); tests pass it explicitly
// since node never calls loadCards().
function ocrRecognizeSlot(imageData, slot, cardList) {
  const raw = ocrSlotRaw(imageData, slot);
  const list = cardList || CARDS;
  const badgeAge = readAgeBadge(imageData, slot);
  const resolved = resolveOCRString(raw, list, badgeAge);
  const nameHint = resolved ? null : (matchOCRName(raw, list) || {}).group;
  return {
    raw,
    cardId: resolved ? resolved.id : null,
    badgeAge,
    nameHint: nameHint ? nameHint[0].name : null,
  };
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
 * rectangles and reads the printed name out of each one. A slot that
 * doesn't resolve to exactly one card comes back null rather than a guess
 * — the caller (renderFullRowStep) pre-fills what it recognises and leaves
 * the rest for the user to type.
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
  // nameHints runs parallel to row: null wherever row[i] resolved (nothing
  // left to hint at), and the recognised base name wherever OCR matched a
  // name but couldn't settle which age copy — the caller
  // (renderFullRowStep) prefills that name so the user only has to pick the
  // age, instead of a blank input.
  const row = [], nameHints = [];
  found.slots.forEach((slot) => {
    // A slot the geometry pass found by measurement rather than by seeing
    // card art in it IS the empty row position — null is exactly how the
    // row model spells that, and reading text out of blank felt would only
    // invent one.
    if (slot.empty) { row.push(null); nameHints.push(null); return; }
    const ocr = ocrRecognizeSlot(pixels, slot, CARDS);
    row.push(ocr.cardId || null);
    nameHints.push(ocr.cardId ? null : ocr.nameHint);
  });
  return { row, nameHints, rivalStr: null, rivalCulture: null, militaryDraws: [] };
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
    fullRowNameHints: null, // parallel to fullRowDraft: recognised name, age not yet settled
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

  // "Name known, age unknown" (Job 2): OCR read the printed name cleanly
  // but the name is shared by 2-4 age copies and the badge couldn't (or
  // didn't) settle which one — see readAgeBadge/resolveOCRString. Rather
  // than hand the user a blank input, prefill the recognised name so the
  // autocomplete already lists just that family (e.g. "Rich Land (A)" /
  // "(I)" / "(II)") and picking the right one is a tap, not typing.
  const nameHint = !draft[cursor] && state.flow.fullRowNameHints ? state.flow.fullRowNameHints[cursor] : null;
  if (nameHint) {
    container.appendChild(makeSub(`Name recognised as "${nameHint}" — scan couldn't tell the age; pick it below.`));
  }

  const { input, suggest } = makeAutocompleteRow('type card, Enter to place');
  container.appendChild(input.wrap);
  setupAutocomplete(input.el, suggest, () => ROW_POOL, (c) => {
    draft[cursor] = c.id;
    if (state.flow.fullRowNameHints) state.flow.fullRowNameHints[cursor] = null;
    fullRowAdvance();
  });
  if (nameHint) {
    input.el.value = nameHint;
    input.el.dispatchEvent(new Event('input'));
  }
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
      let filled = 0, hinted = 0;
      result.row.forEach((id, i) => {
        if (id) { state.flow.fullRowDraft[i] = id; filled++; return; }
        const hint = result.nameHints && result.nameHints[i];
        if (hint) { state.flow.fullRowNameHints[i] = hint; hinted++; }
      });
      const firstUnknown = state.flow.fullRowDraft.findIndex((x) => x === null);
      state.flow.fullRowCursor = firstUnknown === -1 ? state.flow.fullRowCursor : firstUnknown;
      state.flow.fullRowScanStatus = filled === 13
        ? 'Recognised all 13 of 13 from the screenshot.'
        : `Recognised ${filled} of 13 from the screenshot` +
          (hinted ? `, and the name (age not settled) on ${hinted} more` : '') +
          ' — type the rest; each one you type teaches it for next time.';
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
  state.flow.step = 'fullrow';
  state.flow.fullRowDraft = state.row.slice(0, 13);
  while (state.flow.fullRowDraft.length < 13) state.flow.fullRowDraft.push(null);
  state.flow.fullRowNameHints = new Array(13).fill(null);
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
    findRowSlots,
    findTextLines, segmentGlyphs, glyphToGrid, gridDistance, classifyGlyph,
    lineColInk, zeroInkBoxes, median, // exposed for the atlas-harvest tooling only (see appseg_notes.txt)
    ocrSlotRaw, ocrNormalize, levenshtein, matchOCRName, resolveOCRString, ocrRecognizeSlot,
    isBadgeInk, readAgeBadge,
    GLYPH_W, GLYPH_H, OCR_ATLAS,
  };
} else {
  boot();
}
