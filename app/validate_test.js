'use strict';
const { validateRow, AGE_ORDER } = require('./app.js');
const cards = require('./cards.json');
const byId = new Map(cards.map((c) => [c.id, c]));

function row13(ids) {
  const r = ids.slice(0, 13);
  while (r.length < 13) r.push(null);
  return r;
}

// Find a few distinct age-I and age-A civil (rowEligible) cards for legal fixtures.
const ageI = cards.filter((c) => c.rowEligible && c.age === 'I').map((c) => c.id);
const ageA = cards.filter((c) => c.rowEligible && c.age === 'A').map((c) => c.id);

let pass = 0, fail = 0;
function check(name, cond, extra) {
  if (cond) { pass++; console.log(`PASS: ${name}`); }
  else { fail++; console.log(`FAIL: ${name}` + (extra ? ' -- ' + extra : '')); }
}

// Fixture 1: Paul's exact junk row -- Alchemy, Drama x3, Democracy x2, rest empty.
// Must BLOCK on age span and NAME Democracy.
{
  const row = row13(['alchemy', 'drama', 'drama', 'drama', 'democracy', 'democracy']);
  const r = validateRow(row, byId);
  check('1. junk row blocks on age span', !!r.ageIssue, JSON.stringify(r));
  check('1. junk row message names Democracy', r.ageIssue && r.ageIssue.message.includes('Democracy'), r.ageIssue && r.ageIssue.message);
  console.log('    message: ' + (r.ageIssue && r.ageIssue.message));
}

// Fixture 2: legal all-age-I row -- must pass clean (no ageIssue, no dupIssue).
{
  const row = row13(ageI.slice(0, 6));
  const r = validateRow(row, byId);
  check('2. all-age-I row passes clean', !r.ageIssue && !r.dupIssue, JSON.stringify(r));
}

// Fixture 3: legal A+I changeover row -- must pass clean. This is the case a
// naive "all one age" check would wrongly reject.
{
  const row = row13(ageA.slice(0, 3).concat(ageI.slice(0, 3)));
  const r = validateRow(row, byId);
  check('3. A+I changeover row passes clean', !r.ageIssue && !r.dupIssue, JSON.stringify(r));
}

// Fixture 4: three copies of one card, all one age -- must WARN, not block.
{
  const oneCard = ageI[0];
  const row = row13([oneCard, oneCard, oneCard]);
  const r = validateRow(row, byId);
  check('4. triple-copy row does not block', !r.ageIssue, JSON.stringify(r));
  check('4. triple-copy row warns on duplicates', !!r.dupIssue, JSON.stringify(r));
  console.log('    message: ' + (r.dupIssue && r.dupIssue.message));
}

// Fixture 5: all-empty row -- must not crash; document behavior (no block, no warning).
{
  const row = row13([]);
  let threw = false, r;
  try { r = validateRow(row, byId); } catch (e) { threw = true; console.log('    threw: ' + e.message); }
  check('5. all-empty row does not crash', !threw);
  check('5. all-empty row has no ageIssue/dupIssue', r && !r.ageIssue && !r.dupIssue, JSON.stringify(r));
}

// Sanity: a 3-age-wide row (A + II) still blocks even though not literally the
// spec's own example -- exercises the same code path with a different gap.
{
  const ageII = cards.filter((c) => c.rowEligible && c.age === 'II').map((c) => c.id);
  const row = row13(ageA.slice(0, 2).concat(ageII.slice(0, 1)));
  const r = validateRow(row, byId);
  check('6. A+II (span 2) blocks', !!r.ageIssue, JSON.stringify(r));
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
