# The gauntlet: dated reference points for the live league's own progress

These files are a different collection from the rest of `analysis/frozen/`
(read that directory's own `README.md` first — it explains what the *other*
frozen vectors are for, and the trap of measuring a lever a stale vector has
no socket for). This subdirectory exists for one purpose only: giving
`climb` (`rust/src/bin/climb.rs`) something dated and unmoving to compare the
live champion against, because the anchor it already compares against
(`DEFAULT_WEIGHTS`) saturated long ago — see `docs/RUST_LEAGUE.md`'s "The
anchor number is not a strength ladder" for the measurement that showed it.

## The rule

**Frozen forever. Append-only.** A file already here is never edited,
overwritten or replaced — copying a newer snapshot over an existing name
would silently invalidate every gauntlet score already logged against it,
the exact failure mode `docs/HAZARDS.md` and `analysis/frozen/README.md`
both warn about for the older frozen vectors. When the vocabulary or the
lineage has moved far enough that a new reference point is worth having,
*add* a new dated file; never touch an old one.

## Naming

`champion_{2,3,4}p_gen<N>_<K>key_<date>.json`, copied verbatim from
`experiments/rust_champion_{2,3,4}p.json` (gitignored, live-only) at the
moment named. The generation and key count are read off the file's own
`gen` field and `weights` object at copy time, not guessed — see
`analysis/frozen/README.md`'s "name that cannot go stale silently" rule,
which this borrows.

| file | gen | keys | cut |
|---|---|---|---|
| `champion_2p_gen1454_140key_2026-08-06.json` | 1454 | 140 | 2026-08-06 |
| `champion_3p_gen1384_140key_2026-08-06.json` | 1384 | 140 | 2026-08-06 |
| `champion_4p_gen448_140key_2026-08-06.json` | 448 | 140 | 2026-08-06 |
| `champion_2p_gen19554_140key_2026-08-08.json` | 19554 | 140 | 2026-08-08 |
| `champion_3p_gen12364_140key_2026-08-08.json` | 12364 | 140 | 2026-08-08 |
| `champion_4p_gen4034_140key_2026-08-08.json` | 4034 | 140 | 2026-08-08 |
| `champion_2p_gen157130_186key_2026-09-05.json` | 157130 | 186 | 2026-09-05 |
| `champion_3p_gen92751_186key_2026-09-05.json` | 92751 | 186 | 2026-09-05 |
| `champion_4p_gen27546_186key_2026-09-05.json` | 27546 | 186 | 2026-09-05 |

The 09-05 cut is the first one taken on the **186-key** basis; everything above
it is 140-key. That is not a naming detail — a gauntlet member can only be
scored on the vocabulary it was frozen with, so the older files measure the
lineage's progress and the newer one measures its current strength, and the two
are answering different questions. It was added because the 08-08 "late" cut had
stopped being the same-strength sparring partner it was appended to be: 2p moved
19,554 → 157,130 generations between them. `experiments/rust_league.sh` points
`--gauntlet-search` at this cut, since the search hybrid is the one panel member
that actually vetoes.

## These are snapshots, not "the champion"

A file here is a dated past self and a legitimate sparring partner. It is
**not** the current champion, and it goes stale fast — the lineage moved
~9,000 generations at 2p between the two cuts above. Measure the live
`experiments/rust_champion_*.json` before drawing any conclusion about the
running arms.

Learned the hard way on 2026-08-08: `champion_3p_gen1384` was diagnosed as a
bad optimum (two weights pinned at `climb.rs`'s `CLAMP = 60.0`, mirror culture
97.9 against untuned defaults' 152.8) and a plan was formed to reseed the 3p
arm from the 2p vector on the strength of it. Re-measuring against the *live*
3p champion overturned that completely: it had walked back off the clamp on
its own (`pact_partner_lead` -60.00 → -4.21, `uprising` -56.79 → -0.16),
mirrored at 149.2, and **beat** the live 2p vector at a 3p table (2p wins only
19.5% against a 33.3% null). The reseed would have made things worse. See
`docs/AGREEMENT_FIT.md` for the other reversal from the same day.

## What this does and does not prove

Read `docs/RUST_LEAGUE.md`'s "What this does and does not prove" in full
before quoting a gauntlet number. Short version: a rising gauntlet score
means the current champion beats a specific, dated past version of the
lineage by more than that past version did — genuinely comparable over time,
which the anchor no longer is. It is not an external strength measurement:
every file here was produced by the same climb, against the same anchor,
with the same mutation operator and accept gate as the champion being
measured, so a bias the whole lineage shares is invisible to it by
construction (`docs/HAZARDS.md`'s "no external anchor" hazard). It narrows
what anchor saturation can hide; it does not remove the hazard.
