"""Sanity checks for the TTA base-2015 card data files.

Run from the repo root:  python3 data/validate_cards.py
Checks JSON validity, required fields, duplicate (name, age) pairs, monotonic
2p<=3p<=4p counts (pacts excepted: they are removed entirely in 2-player games),
and per-age deck sizes against the rulebook / published component counts
(military 10/45/50/45 = 150; civil 20 + 3x53 = 179).
"""
import json, collections, sys

mil = json.load(open('data/cards_military_actions.json'))
civ = json.load(open('data/cards_civil.json'))
wl  = json.load(open('data/cards_wonders_leaders.json'))
errs, warns = [], []

# --- structural checks on the military/actions file ---
seen = set()
for c in mil['cards']:
    for k in ('name', 'age', 'type', 'deck', 'count'):
        if k not in c: errs.append(f"{c.get('name','?')}: missing {k}")
    if c.get('age') not in ('A','I','II','III'): errs.append(f"{c.get('name')}: bad age {c.get('age')}")
    if c.get('deck') not in ('military','civil'): errs.append(f"{c.get('name')}: bad deck {c.get('deck')}")
    cnt = c.get('count', {})
    if set(cnt) != {'2p','3p','4p'} or not all(isinstance(v,int) and v>=0 for v in cnt.values()):
        errs.append(f"{c.get('name')}: bad count {cnt}")
    if cnt.get('2p',0) > cnt.get('3p',0) or cnt.get('3p',0) > cnt.get('4p',0):
        if c.get('type') != 'pact':
            errs.append(f"{c.get('name')} ({c['age']}): non-monotonic count {cnt}")
    key = (c.get('name'), c.get('age'))
    if key in seen: errs.append(f"duplicate card {key}")
    seen.add(key)
    if c.get('deck') == 'civil' and c.get('type') != 'action':
        errs.append(f"{c['name']}: civil deck but type {c['type']}")

# --- pacts removed in 2p ---
for c in mil['cards']:
    if c['type'] == 'pact' and c['count']['2p'] != 0:
        errs.append(f"pact {c['name']} has non-zero 2p count")

# --- military deck sizes ---
target_mil = {'A': (10,10,10), 'I': (43,45,45), 'II': (46,50,50), 'III': (41,45,45)}
mt = collections.Counter()
for c in mil['cards']:
    if c['deck'] == 'military':
        for p in ('2p','3p','4p'): mt[(c['age'],p)] += c['count'][p]
for age,(a2,a3,a4) in target_mil.items():
    got = (mt[(age,'2p')], mt[(age,'3p')], mt[(age,'4p')])
    print(f"military {age}: {got}  expected {(a2,a3,a4)}", "OK" if got==(a2,a3,a4) else "MISMATCH")
    if got != (a2,a3,a4): errs.append(f"military deck {age} size {got} != {(a2,a3,a4)}")
tot4 = sum(mt[(a,'4p')] for a in ('A','I','II','III'))
print("military total 4p:", tot4, "(box: 150)")
if tot4 != 150: errs.append(f"military total {tot4} != 150")

# --- civil deck sizes: techs + wonders/leaders + action cards ---
ct = collections.Counter()
for c in civ['cards']:
    for p in ('2p','3p','4p'): ct[(c['age'],p)] += c['count'][p]
for c in mil['cards']:
    if c['deck'] == 'civil':
        for p in ('2p','3p','4p'): ct[(c['age'],p)] += c['count'][p]
wlc = collections.Counter()
for c in wl['cards']:
    wlc[c['age']] += 1
    for p in ('2p','3p','4p'): ct[(c['age'],p)] += 1
target_civ = {'A': (20,20,20), 'I': (44,50,53), 'II': (44,50,53), 'III': (44,50,53)}
for age,(a2,a3,a4) in target_civ.items():
    got = (ct[(age,'2p')], ct[(age,'3p')], ct[(age,'4p')])
    print(f"civil {age}: {got}  expected {(a2,a3,a4)}  (wonders+leaders {wlc[age]})",
          "OK" if got==(a2,a3,a4) else "MISMATCH")
    if got != (a2,a3,a4): warns.append(f"civil deck {age} size {got} != {(a2,a3,a4)}")
print("civil total 4p:", sum(ct[(a,'4p')] for a in ('A','I','II','III')), "(box: 179)")

# --- scoring events ---
se = [c['name'] for c in mil['cards'] if c.get('scoringEvent')]
nonIII = [c['name'] for c in mil['cards'] if c.get('scoringEvent') and c['age']!='III']
missing = [c['name'] for c in mil['cards'] if c['age']=='III' and c['type']=='event' and not c.get('scoringEvent')]
print(f"scoring events: {len(se)} (all Age III events: {len(missing)==0 and not nonIII})")
if nonIII: errs.append(f"scoringEvent on non-Age-III cards: {nonIII}")
if missing: errs.append(f"Age III events not marked scoringEvent: {missing}")

print("\nERRORS:", *errs, sep="\n  " if errs else " none")
print("WARNINGS:", *warns, sep="\n  " if warns else " none")

sys.exit(1 if errs else 0)
