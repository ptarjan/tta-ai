"""The four human archetypes, and the corpus segments they are fitted to.

WHERE THESE COME FROM
---------------------
`tools/bgo_cluster.py` on the 1,011-game BGO corpus.  Its headline result is a
NEGATIVE one and it decided the design of this file: at 2p, k-means silhouette
over twelve behavioural axes is 0.10-0.14 for k=2..6 against a permutation
null of 0.08-0.11 -- a ratio of 1.03-1.37.  **Human play is a continuum, not a
set of types.**  There is exactly one genuinely discrete behaviour in the
corpus, war (83% of 2p players declare zero, all game), and everything else is
a direction in a single blob.

So these are SEGMENTS, cut by the rule in `tools/human_fit.py:segment()`
(auditable, at the corpus's own quantiles) rather than centroids, and they are
named for the direction they sit at the end of.  k-means at k=3 and k=5
recovers the same directions, which is the only sense in which four is the
right number.

    segment    2p rows          what it is                        wins
    ---------  ---------------  --------------------------------  ----
    builder    534 (38.6%)      the modal human: 33 cards,          40%
                                8.1 wonder stages, no wars
    wonder     307 (22.2%)      12.7 stages, 3.76 wonders,          53%
                                highest score in the corpus (184)
    tempo      194 (14.0%)      39.7 cards, 7.2 stages, the         57%
                                card-throughput end
    warlord    237 (17.1%)      1.48 wars + 1.46 aggressions        73%
    passive    111 ( 8.0%)      27.6 cards, 4.5 stages, score 115   26%

**`passive` is deliberately not built.**  It is the losing tail of the
distribution (26% win share against a 50% null); a bot fitted to it would be
decoration in the pool, and `docs/UNATTENDED.md` trap 2 is about exactly that.

**The win-share column is endogenous and must not be read as "war is good".**
The segments are cut on behaviour that is itself downstream of who is winning
-- a human declares war *because* they are ahead and want to close, so the
warlord segment's 73% is largely selection.  `docs/HUMAN_BOTS.md` measures
what these bots actually do against each other, which is the non-endogenous
version of that question.

SKILL DOES PREDICT STYLE, BUT NOT IN THE USEFUL DIRECTION
---------------------------------------------------------
Segment mix by BGO level (2p rows):

    level      builder  wonder  tempo  warlord  passive
    Emperor      37.5%   19.3%  15.0%    18.1%    10.1%
    King         35.3%   21.6%  17.2%    21.1%     4.9%
    Prince       42.7%   27.0%  16.2%     9.7%     4.3%
    Warlord      40.7%   25.6%   9.0%    16.9%     7.8%

Stronger players are MORE militarist and take more cards; weaker players build
more wonders.  Combined with `docs/HUMAN_BASELINE.md`'s finding that Emperor
games score *lower* than Prince games, the corpus gives no clean "imitate the
best players" target, so every TARGET below is fitted on the whole 2p corpus
segment rather than an Emperor-only slice.  The Emperor-only targets are
within 1 human sd of these on every axis except wars.
"""
from __future__ import annotations

from .base import HumanBot

__all__ = ["HumanBuilderBot", "HumanWonderBot", "HumanTempoBot",
           "HumanWarlordBot"]


#: Knobs every archetype's fit is allowed to move, and the grid it may move
#: them over.  Kept small on purpose: 24-game measurements have a real SE on
#: every axis, so a large search space would fit noise.  See
#: `tools/human_fit.py`'s "Noise" note.
_COMMON_KNOBS = {
    "take_bias": [0.0, 4.0, 8.0, 11.0, 15.0, 20.0],
    "take_first_p": [0.0, 0.15, 0.3, 0.45, 0.6, 0.75, 0.9],
    "price_scale": [0.35, 0.5, 0.7, 1.0],
    "hand_free": [0, 2, 4, 6],
    "wonder_appetite": [0.6, 1.0, 1.5, 2.2, 3.0, 4.0],
    "wonder_finish_bias": [0.0, 3.0, 8.0],
    "gov_min_age": [0, 1, 2],
    "revolution_round_min": [1, 6, 9, 11, 13],
    "revolution_round_max": [8, 11, 13, 16],
    "revolution_min": [6.0, 10.0, 16.0],
    "agg_rate": [0.02, 0.05, 0.10, 0.20, 0.40, 0.70, 1.0],
    "agg_centre": [1.0, 3.0, 4.5, 6.0],
    "war_rate": [0.01, 0.03, 0.08, 0.20, 0.50, 1.0],
    "war_centre": [1.0, 3.0, 4.5, 6.0],
}

#: Shared starting point.  `take_temp` and `noise` are NOT fitted: they are the
#: anti-exploit knobs and they are set from the outside (a fit would drive them
#: to zero, because determinism always matches a mean better than a draw does).
_BASE = {
    "take_temp": 3.0,
    "noise": 1.5,
    "revolution_round_min": 9,
    "gov_min_age": 2,
    "war_width": 2.5,
    "agg_width": 2.5,
    "endgame_mil_boost": 1.6,
    "max_take_cost": {0: 2, 1: 2, 2: 3, 3: 3, 4: 3},
    "three_ca_min_actions": 5,
}


#: Per-archetype grids, narrowed to the knobs that move that archetype's
#: DEFINING axes.  The wide `_COMMON_KNOBS` grid was tried first and is left
#: in as the record: at 72 games a pass over 14 knobs takes ~15 minutes and
#: moved the builder loss by less than its own block-to-block noise
#: (0.157 -> 0.186 -> 0.171 on re-measurement of the SAME knobs).  Fitting a
#: knob whose effect is smaller than the measurement noise is fitting noise,
#: which is the thing docs/UNATTENDED.md keeps warning about.
_WONDER_KNOBS = {k: _COMMON_KNOBS[k] for k in
                 ("wonder_appetite", "wonder_finish_bias", "take_bias",
                  "price_scale", "hand_free")}
_WAR_KNOBS = {k: _COMMON_KNOBS[k] for k in
              ("war_rate", "war_centre", "agg_rate", "agg_centre")}


class _Arch(HumanBot):
    FIT_KNOBS = _COMMON_KNOBS


class HumanBuilderBot(_Arch):
    """The modal human: broad economy, ~3 wonders, essentially never fights."""

    NAME = "hum:builder"
    N_ROWS = 534
    TARGET = {
        "score": 156.48, "wonders_completed": 2.571, "wonder_stages": 8.114,
        "takes": 33.00, "tier3_pct": 4.482, "wars_declared": 0.0,
        "aggressions": 0.584, "first_gov_round": 11.972, "sci_final": 15.528,
        "bids": 3.243, "leaders_elected": 3.654, "colonies": 1.579,
        "rounds": 19.361,
    }
    PROFILE = dict(_BASE, **{
        "take_bias": 11.0,
        "take_first_p": 0.45,
        "price_scale": 0.5,
        "hand_free": 4,
        "wonder_appetite": 2.2,
        "wonder_finish_bias": 3.0,
        "revolution_round_max": 18,
        "revolution_min": 10.0,
        "agg_rate": 0.10,
        "agg_centre": 4.0,
        "war_rate": 0.03,
        "war_centre": 6.0,
    })


class HumanWonderBot(_Arch):
    """12.7 stages, 3.76 wonders -- the highest-scoring segment in the corpus.

    NOT the same bot as `var:wonder`, which the roster includes *because*
    experts call wonder spam a noob trap: that one is a hand-tuned Michelangelo
    line that completes 1.67 wonders a game.  This one is fitted to what the
    humans who actually do it actually achieve.
    """

    NAME = "hum:wonder"
    N_ROWS = 307
    TARGET = {
        "score": 183.75, "wonders_completed": 3.759, "wonder_stages": 12.733,
        "takes": 35.687, "tier3_pct": 4.593, "wars_declared": 0.0,
        "aggressions": 0.401, "first_gov_round": 11.953, "sci_final": 17.756,
        "bids": 3.381, "leaders_elected": 3.687, "colonies": 1.518,
        "rounds": 19.482,
    }
    FIT_WEIGHTS = {"wonder_stages": 3.0, "wonders_completed": 3.0}
    FIT_KNOBS = _WONDER_KNOBS
    PROFILE = dict(_BASE, **{
        # fitted: loss 0.47 -> 0.37; stages 9.3 -> 10.1 against a corpus 12.7
        "take_bias": 15.0,
        "take_first_p": 0.45,
        "price_scale": 0.35,
        "hand_free": 4,
        "wonder_appetite": 4.0,
        "wonder_finish_bias": 8.0,
        "revolution_round_max": 18,
        "revolution_min": 10.0,
        "agg_rate": 0.05,
        "agg_centre": 4.5,
        "war_rate": 0.01,
        "war_centre": 6.0,
    })


class HumanTempoBot(_Arch):
    """39.7 cards a game -- the card-throughput end of the corpus.

    The axis `docs/HUMAN_BASELINE.md` calls "the cleanest single behavioural
    finding": our champion takes 24.2 cards and pays 3 CA for 22.4% of them.
    This bot takes 40 and pays 3 CA for ~4%.
    """

    NAME = "hum:tempo"
    N_ROWS = 194
    TARGET = {
        "score": 156.07, "wonders_completed": 2.423, "wonder_stages": 7.222,
        "takes": 39.742, "tier3_pct": 4.279, "wars_declared": 0.0,
        "aggressions": 0.526, "first_gov_round": 11.417, "sci_final": 16.072,
        "bids": 3.505, "leaders_elected": 3.897, "colonies": 1.608,
        "rounds": 19.201,
    }
    FIT_WEIGHTS = {"takes": 3.0, "tier3_pct": 1.5}
    PROFILE = dict(_BASE, **{
        "take_bias": 20.0,
        "take_first_p": 0.15,
        "price_scale": 0.7,
        "hand_free": 6,
        "wonder_appetite": 1.5,
        "wonder_finish_bias": 3.0,
        "revolution_round_max": 18,
        "revolution_min": 6.0,
        "agg_rate": 0.10,
        "agg_centre": 4.0,
        "war_rate": 0.03,
        "war_centre": 6.0,
    })


class HumanWarlordBot(_Arch):
    """The one discrete behaviour in the corpus: 1.48 wars, 1.46 aggressions.

    The point of this bot is that it is the DIRECT replacement for the pool's
    known exploit.  `var:military` fires on a hard `+3 strength lead` and the
    champion holds it under that on 94.5% of turns.  Here the same behaviour is
    a logistic in the lead with width 2.5, so suppression is gradual: see
    docs/HUMAN_BOTS.md for the measured trigger rate against the champion.
    """

    NAME = "hum:warlord"
    N_ROWS = 237
    TARGET = {
        "score": 158.35, "wonders_completed": 2.599, "wonder_stages": 8.359,
        "takes": 33.928, "tier3_pct": 4.532, "wars_declared": 1.481,
        "aggressions": 1.460, "first_gov_round": 11.224, "sci_final": 13.173,
        "bids": 3.021, "leaders_elected": 3.751, "colonies": 1.363,
        "rounds": 19.608,
    }
    #: score is EXCLUDED from the warlord fit.  `tools/bgo_botmatch.py`
    #: measures a MIRROR table, so a warlord fit on score is being asked to
    #: reproduce a corpus number (158) that was earned by militarists playing
    #: mostly non-militarists.  Two warring bots take each other's culture and
    #: land near 90 whatever their knobs say; fitting score would just turn the
    #: war off, which is the one thing this bot exists to do.  The mirror
    #: confound is measured in docs/HUMAN_BOTS.md.
    FIT_WEIGHTS = {"wars_declared": 3.0, "aggressions": 2.0, "score": 0.0,
                   "sci_final": 0.0}
    FIT_KNOBS = _WAR_KNOBS
    PROFILE = dict(_BASE, **{
        "take_bias": 11.0,
        "take_first_p": 0.45,
        "price_scale": 0.5,
        "hand_free": 4,
        "wonder_appetite": 2.2,
        "wonder_finish_bias": 3.0,
        "revolution_round_max": 18,
        "revolution_min": 10.0,
        "mil_stance": "top2",
        "mil_margin": 2,
        # fitted (tools/human_fit.py, 72 games/eval, 3 passes):
        # loss 1.25 -> 0.50, wars 0.38 -> 1.56 against a corpus 1.48.
        "agg_rate": 0.40,
        "agg_centre": 3.0,
        "war_rate": 1.0,
        "war_centre": 1.0,
        "war_from_age": 1,
        "unit_cap": {0: 3, 1: 5, 2: 7, 3: 8, 4: 8},
    })
