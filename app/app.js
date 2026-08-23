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
 * OCR seam. NOT implemented — no camera, no OCR library, per spec.
 * Anything that later fills this in just has to return the same shape
 * manual entry produces, and the rest of the app does not change.
 * ------------------------------------------------------------------- */
async function ocrScanSeam(_imageBlobOrDataUrl) {
  throw new Error('OCR not implemented: this is a stub seam. Fill in to return ' +
    '{ row: [13 card ids or null], rivalStr: N, rivalCulture: N, militaryDraws: [ids] }');
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

  const { input, suggest } = makeAutocompleteRow('type card, Enter to place');
  container.appendChild(input.wrap);
  setupAutocomplete(input.el, suggest, () => ROW_POOL, (c) => { draft[cursor] = c.id; fullRowAdvance(); });
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
  module.exports = { validateRow, AGE_ORDER, matchScore, searchCards };
} else {
  boot();
}
