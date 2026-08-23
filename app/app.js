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
window.ocrScanSeam = ocrScanSeam;

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
 * State + persistence
 * ------------------------------------------------------------------- */
const STORAGE_KEY = 'ttaapp_state_v1';

function freshState() {
  return {
    v: 1,
    row: new Array(13).fill(null),
    dropCount: 0,
    pendingNew: [],
    pendingMilitary: [],
    hand: { civil: [], military: [] },
    playedTotal: 0,
    takenTotal: 0,
    rival: { str: 0, culture: 0 },
    seed: Math.floor(Math.random() * 1e9),
    wasmState: null,
    moves: [],
    positionText: '',
    history: [],
  };
}

let state = null;

function save() {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state)); } catch (e) { /* ignore quota errors */ }
}

function load() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch (e) { /* ignore corrupt state */ }
  return null;
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
 * DOM refs
 * ------------------------------------------------------------------- */
const $ = (id) => document.getElementById(id);
const el = {
  banner: $('fakeBanner'),
  stats: $('stats'),
  takenPlayed: $('takenPlayed'),
  undoBtn: $('undoBtn'),
  newGameBtn: $('newGameBtn'),
  rowStrip: $('rowStrip'),
  dropMinus: $('dropMinus'),
  dropPlus: $('dropPlus'),
  dropCount: $('dropCount'),
  dropPreview: $('dropPreview'),
  fullRowBtn: $('fullRowBtn'),
  newCardInput: $('newCardInput'),
  newCardSuggest: $('newCardSuggest'),
  newCardChips: $('newCardChips'),
  milCardInput: $('milCardInput'),
  milCardSuggest: $('milCardSuggest'),
  milCardChips: $('milCardChips'),
  strMinus: $('strMinus'), strPlus: $('strPlus'), strVal: $('strVal'),
  cultMinus: $('cultMinus'), cultPlus: $('cultPlus'), cultVal: $('cultVal'),
  applyBtn: $('applyBtn'),
  adviceTop: $('adviceTop'),
  adviceRest: $('adviceRest'),
  handChips: $('handChips'),
  fullRowModal: $('fullRowModal'),
  fullRowSlots: $('fullRowSlots'),
  fullRowInput: $('fullRowInput'),
  fullRowSuggest: $('fullRowSuggest'),
  fullRowEmpty: $('fullRowEmpty'),
  fullRowCancel: $('fullRowCancel'),
  fullRowDone: $('fullRowDone'),
};

let dropMode = false; // when true, tapping a row chip sets dropCount instead of taking

/* ---------------------------------------------------------------------
 * Rendering
 * ------------------------------------------------------------------- */
function renderAll() {
  el.banner.classList.toggle('hidden', !Engine.usingStub);
  el.stats.textContent = state.positionText || 'round – · age – · CA – MA – food – res – sci –';

  const gap = state.takenTotal - state.playedTotal;
  el.takenPlayed.textContent = `taken ${state.takenTotal} / played ${state.playedTotal}`;
  el.takenPlayed.classList.toggle('warn', gap >= 3);

  renderRow();
  renderChipList(el.newCardChips, state.pendingNew, 'new', removePendingNew);
  renderChipList(el.milCardChips, state.pendingMilitary, 'mil', removePendingMilitary);

  el.dropCount.textContent = state.dropCount;
  el.strVal.textContent = state.rival.str;
  el.cultVal.textContent = state.rival.culture;

  renderAdvice();
  renderHand();
  save();
}

function renderRow() {
  el.rowStrip.innerHTML = '';
  state.row.forEach((id, i) => {
    const div = document.createElement('div');
    div.className = 'slotChip' + (id ? '' : ' empty') + (dropMode && i < state.dropCount ? ' dropping' : '');
    div.innerHTML = `<span class="slotNum">slot ${i + 1}</span><span>${cardDisplay(id)}</span>`;
    div.addEventListener('click', () => onRowChipTap(i));
    el.rowStrip.appendChild(div);
  });
  const dropped = state.row.slice(0, state.dropCount).map(cardDisplay).filter((n) => n !== '.');
  el.dropPreview.textContent = state.dropCount
    ? `removing from left: ${dropped.length ? dropped.join(', ') : '(all empty slots)'}`
    : '';
}

function onRowChipTap(i) {
  if (dropMode) {
    state.dropCount = i;
    renderAll();
    return;
  }
  const id = state.row[i];
  if (!id) return;
  snapshotForUndo();
  state.row.splice(i, 1);
  state.row.push(null);
  state.hand.civil.push(id);
  state.takenTotal += 1;
  renderAll();
}

function renderChipList(container, ids, kind, onRemove) {
  container.innerHTML = '';
  ids.forEach((id, i) => {
    const chip = document.createElement('div');
    chip.className = 'chip' + (kind === 'mil' ? ' mil' : '');
    chip.innerHTML = `${cardDisplay(id)}<span class="x">×</span>`;
    chip.addEventListener('click', () => onRemove(i));
    container.appendChild(chip);
  });
}

function removePendingNew(i) { state.pendingNew.splice(i, 1); renderAll(); }
function removePendingMilitary(i) { state.pendingMilitary.splice(i, 1); renderAll(); }

function renderAdvice() {
  const moves = state.moves || [];
  if (!moves.length) {
    el.adviceTop.textContent = 'no advice yet — apply a turn';
    el.adviceRest.innerHTML = '';
    return;
  }
  // The engine auto-plays the whole turn and reports the moves it PLAYED, in
  // order -- these are consecutive steps, not alternatives to choose between.
  // Rendering move 2 as a runner-up would read as "or do this instead" and
  // lose the rest of the turn, so every step is shown, numbered, all of them.
  // Scores are 0.0 by construction (each move was the top candidate at its own
  // decision point), so there is nothing informative to display.
  const top = moves[0];
  el.adviceTop.innerHTML = `<span class="step">1</span>${top.text}` +
    (top.detail ? `<span class="detail">${top.detail}</span>` : '');
  el.adviceRest.innerHTML = '';
  moves.slice(1).forEach((m, i) => {
    const div = document.createElement('div');
    div.className = 'move';
    div.innerHTML = `<span class="step">${i + 2}</span>${m.text}` +
      (m.detail ? `<span class="detail">${m.detail}</span>` : '');
    el.adviceRest.appendChild(div);
  });
}

function renderHand() {
  el.handChips.innerHTML = '';
  const addGroup = (ids, kind) => {
    ids.forEach((id, i) => {
      const chip = document.createElement('div');
      chip.className = 'chip' + (kind === 'military' ? ' mil' : '');
      chip.textContent = cardDisplay(id);
      chip.addEventListener('click', () => markPlayed(kind, i));
      el.handChips.appendChild(chip);
    });
  };
  addGroup(state.hand.civil, 'civil');
  addGroup(state.hand.military, 'military');
}

function markPlayed(kind, i) {
  snapshotForUndo();
  const [id] = state.hand[kind].splice(i, 1);
  if (kind === 'civil') state.playedTotal += 1;
  renderAll();
}

/* ---------------------------------------------------------------------
 * Autocomplete wiring
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
 * Turn commit + advisor call
 * ------------------------------------------------------------------- */
function buildRowLine(row) {
  const slots = row.slice(0, 13);
  while (slots.length < 13) slots.push(null);
  return 'row ' + slots.map(cardDisplay).join(', ');
}

async function applyTurn(newRow) {
  snapshotForUndo();

  state.row = newRow.slice(0, 13);
  while (state.row.length < 13) state.row.push(null);

  if (state.pendingMilitary.length) {
    state.hand.military.push(...state.pendingMilitary);
  }

  const lines = [buildRowLine(state.row)];
  lines.push(`p1 str=${state.rival.str} c=${state.rival.culture}`);
  if (state.pendingMilitary.length) {
    const civilNames = state.hand.civil.map(cardDisplay).join(', ');
    const milNames = state.hand.military.map(cardDisplay).join(', ');
    lines.push(`p0 hand ${civilNames} | ${milNames}`);
  }

  const request = {
    players: 2,
    seat: 0,
    seed: state.seed,
    state: state.wasmState,
    lines,
  };

  el.applyBtn.disabled = true;
  el.applyBtn.textContent = 'thinking…';
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
  el.applyBtn.disabled = false;
  el.applyBtn.textContent = 'Apply turn & advise';

  state.pendingNew = [];
  state.pendingMilitary = [];
  state.dropCount = 0;
  dropMode = false;
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
 * Full-row modal (turn 1 / full resync escape hatch)
 * ------------------------------------------------------------------- */
let fullRowDraft = [];
let fullRowCursor = 0;

function openFullRowModal() {
  fullRowDraft = state.row.slice(0, 13);
  while (fullRowDraft.length < 13) fullRowDraft.push(null);
  fullRowCursor = fullRowDraft.findIndex((x) => x === null);
  if (fullRowCursor === -1) fullRowCursor = 0;
  renderFullRowSlots();
  el.fullRowModal.classList.remove('hidden');
  el.fullRowInput.value = '';
  el.fullRowInput.focus();
}

function renderFullRowSlots() {
  el.fullRowSlots.innerHTML = '';
  fullRowDraft.forEach((id, i) => {
    const div = document.createElement('div');
    div.className = 'slotChip' + (id ? '' : ' empty') + (i === fullRowCursor ? ' taken' : '');
    div.innerHTML = `<span class="slotNum">${i + 1}</span><span>${cardDisplay(id)}</span>`;
    div.addEventListener('click', () => { fullRowCursor = i; renderFullRowSlots(); });
    el.fullRowSlots.appendChild(div);
  });
}

function fullRowAdvanceCursor() {
  const next = fullRowDraft.findIndex((x, i) => i > fullRowCursor && x === null);
  fullRowCursor = next !== -1 ? next : Math.min(fullRowCursor + 1, 12);
}

/* ---------------------------------------------------------------------
 * Wire everything up
 * ------------------------------------------------------------------- */
function wireUI() {
  el.undoBtn.addEventListener('click', undo);

  el.newGameBtn.addEventListener('click', () => {
    if (!confirm('Start a new game? This clears the current session.')) return;
    state = freshState();
    save();
    renderAll();
    openFullRowModal();
  });

  el.dropMinus.addEventListener('click', () => { state.dropCount = Math.max(0, state.dropCount - 1); dropMode = true; renderAll(); });
  el.dropPlus.addEventListener('click', () => { state.dropCount = Math.min(13, state.dropCount + 1); dropMode = true; renderAll(); });

  el.fullRowBtn.addEventListener('click', openFullRowModal);

  setupAutocomplete(el.newCardInput, el.newCardSuggest, () => ROW_POOL, (c) => {
    state.pendingNew.push(c.id);
    renderAll();
  });
  setupAutocomplete(el.milCardInput, el.milCardSuggest, () => MIL_POOL, (c) => {
    state.pendingMilitary.push(c.id);
    renderAll();
  });
  setupAutocomplete(el.fullRowInput, el.fullRowSuggest, () => ROW_POOL, (c) => {
    fullRowDraft[fullRowCursor] = c.id;
    fullRowAdvanceCursor();
    renderFullRowSlots();
  });

  el.strMinus.addEventListener('click', () => { state.rival.str = Math.max(0, state.rival.str - 1); renderAll(); });
  el.strPlus.addEventListener('click', () => { state.rival.str += 1; renderAll(); });
  el.cultMinus.addEventListener('click', () => { state.rival.culture = Math.max(0, state.rival.culture - 1); renderAll(); });
  el.cultPlus.addEventListener('click', () => { state.rival.culture += 1; renderAll(); });

  el.applyBtn.addEventListener('click', () => {
    const newRow = state.row.slice(state.dropCount).concat(state.pendingNew);
    applyTurn(newRow);
  });

  el.fullRowEmpty.addEventListener('click', () => {
    fullRowDraft[fullRowCursor] = null;
    fullRowAdvanceCursor();
    renderFullRowSlots();
  });
  el.fullRowCancel.addEventListener('click', () => el.fullRowModal.classList.add('hidden'));
  el.fullRowDone.addEventListener('click', () => {
    el.fullRowModal.classList.add('hidden');
    applyTurn(fullRowDraft);
  });
}

/* ---------------------------------------------------------------------
 * Boot
 * ------------------------------------------------------------------- */
async function boot() {
  await loadCards();
  await Engine.init();
  const restored = load();
  state = restored || freshState();
  wireUI();
  renderAll();
  const hasAnyCard = state.row.some((x) => x);
  if (!hasAnyCard && !state.moves.length) openFullRowModal();
}

boot();
