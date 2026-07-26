# data/ progress log

- 2026-07-26: Added all 33 yellow civil ACTION card variants (14 distinct names) to
  data/cards_military_actions.json with type "action", deck "civil" and 2p/3p/4p counts.
  Names + exact effects from the digital-edition localization strings (CivilCards_card_names/
  card_texts), cross-checked vs fandom "Card List: Digital Edition" and faq_v15.pdf p.12.
  Per-age action-card totals 10/13/13/13 derived from the sourced civil deck sizes
  (Age A 20; Ages I/II/III 53 at 4p = 44+6+3; 179 civil cards total).
- 2026-07-26: cards_civil.json — Democracy corrected to 1/2/2 (its extra copy is a "3+" copy,
  not a "4" copy: each Civil deck has exactly 3 cards marked "4" and 6 marked "3+"), which makes
  the Age III civil deck 44/50/53. Flagged the four Age I candidates (Iron/Alchemy/Swordsmen/
  Knights): one of them has 2 copies at 4 players, not 3 — Age I currently totals 54 not 53.
  Documented the Age A civil deck: 20 cards = 6 leaders + 4 wonders + 10 action cards (the six
  Age A technologies are printed on the player boards, hence count 0).
- 2026-07-26: complete=true on cards_military_actions.json (142 cards); added data/validate_cards.py;
  OPEN_QUESTIONS items 1, 2, 6 and 8 resolved, new items 16-18 opened (Age II Breakthrough value,
  action-card copy split within Ages I-III, Age I civil deck off by one).
