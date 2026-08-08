# Supervised-fit weights: one vector, three copies, not a climb lineage

`docs/AGREEMENT_FIT.md`'s deliverable -- the ~140 `WeightKey` weights fit
DIRECTLY to strong-human move choices by streaming multinomial softmax
cross-entropy over `rust/src/bin/agreefit.rs`'s cached decision points
(250 train games, held out on a disjoint 120), NOT by the hill-climb
league `analysis/frozen/gauntlet/` snapshots. Read `docs/AGREEMENT_FIT.md`
first for the numbers and the verdict these vectors exist to support.

**Same freeze-forever, append-only rule as every other subdirectory here**
(`analysis/frozen/README.md`) -- a file already here is never edited or
overwritten; a rerun of `agreefit` gets a new dated name.

## Why a separate subdirectory from `gauntlet/`

`gauntlet/`'s own `README.md` is explicit: those files are "copied verbatim
from `experiments/rust_champion_{2,3,4}p.json` ... at the moment named" --
dated snapshots of the SAME climb lineage, used as `climb`'s own past-self
reference points. These are not that: they were never played a single game
by the league, never survived an accept gate, and carry no `sigma`/
`vs_anchor`/`since_accept` bookkeping (`gen: 0` is a placeholder, not a real
generation count). They ARE loadable by the same `load_weights`/gauntlet A/B
tooling everything else in `analysis/frozen/` is (same JSON shape, same 140
keys, `dominance_repair`-clean), which is the whole point of writing them in
this format -- so a later run can drop one into a gauntlet match against a
real champion and see whether "agrees with humans more" also means "wins
more games". That test has NOT been run yet as of this commit.

## One pooled vector, not three independently fit ones

All three files (`fitted_{2,3,4}p_agreefit_2026-08-08.json`) hold the
IDENTICAL weight vector. `agreefit` pools every player count's decisions
into one training set rather than fitting three separately -- splitting 370
games three ways would leave each player-count fit starved (roughly 90-125
games apiece) for a ~140-dimensional fit. Duplicated across three
player-count-named files purely so existing `players`-keyed loading code
(`WeightedBot` per seat count) can pick one up without a special case.

| file | source | training decisions | held-out top-1 agreement |
|---|---|---|---|
| `fitted_2p_agreefit_2026-08-08.json` | `agreefit`, 2026-08-08 | 59,867 (pooled 2p+3p+4p) | 38.9% (see `docs/AGREEMENT_FIT.md`) |
| `fitted_3p_agreefit_2026-08-08.json` | identical vector | " | " |
| `fitted_4p_agreefit_2026-08-08.json` | identical vector | " | " |
