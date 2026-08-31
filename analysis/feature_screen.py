#!/usr/bin/env python3
"""A CHEAP SCREEN for proposed evaluator features: does held-out R2 rise when
column X is added to the champion's own `phi`?

Reads a `phidump` v2 file (`rust/src/bin/phidump.rs`), which carries the 174
`phi` columns the champion actually scores with plus a block of EXTRA
candidate columns it has no `WeightKey` for. Fits ridge on `phi` alone and on
`phi + X`, on games held out from the fit, and reports the difference.

Three things make the number mean something:

* SPLIT BY GAME. Every decision in a game shares one backfilled label, so a
  row-wise split leaks the answer across the fold. Grouped 5-fold here, and
  the R2 is POOLED over all held-out rows -- each row is held out exactly
  once, so the pooled figure is a single honest number rather than an average
  of five noisy ones.
* A POSITIVE CONTROL, run before any candidate is read. Drop a column the
  evaluator demonstrably uses out of the base set and offer it back as a
  candidate: if the screen cannot recover a feature the bot already scores
  with, it cannot detect a new one either and every number below it is noise.
* A NOISE FLOOR, measured not assumed. Gaussian columns, an EXACT linear
  combination of existing `phi` columns, and REAL candidate columns with
  their row order permuted are screened exactly like real candidates. The
  largest gain any of them scores is the floor; a candidate under it is a
  null however good the story is.
* SAME-SIZE BLOCK NULLS from a SECOND, disjoint gaussian pool
  (`--block-null-cols`). A family of 99 columns has to be read against 99
  noise columns added at once, not against a one-column floor. The pool is
  disjoint from the floor's because the floor is a MAX over its pool, so
  widening that pool would move the floor and stop it being comparable to
  the figure already on record.
* SPANNED-NESS next to every gain. `redundancy` reports how much of a
  candidate the 174 base columns already reconstruct. The screen measures
  UNIQUE signal: a zero gain at spanned 1.0 means "phi already has this",
  not "this does not matter".

## Why cross-products instead of refitting

Every question here is "ridge on some SUBSET of the same columns". Fitting
each subset from the data matrix would be ~200 passes over 900k x 200
float64. Instead each fold's raw cross-product matrix `B'B` (`B = [X, 1, y]`)
is accumulated once, and every subset is then a symmetric solve on a
sub-block of it -- exact, not approximate: ridge's normal equations are a
function of the cross-products alone. Screening a hundred candidates at three
alphas costs one pass over the data and a few hundred 200x200 solves.

usage: feature_screen.py DUMP [--label margin] [--alpha 1.0]
"""

import argparse
import re

import numpy as np

HEADER = 16


def load(path):
    """Records, phi width and extra width from a phidump v1 or v2 file.

    v1 wrote a hard zero where v2 writes `extra`, so taking the width from
    the header (rather than assuming it) reads both.
    """
    with open(path, "rb") as fh:
        head = fh.read(HEADER)
        assert head[:4] == b"TPHI", f"bad magic {head[:4]!r}"
        version, dims, extra = (int(v) for v in np.frombuffer(head, dtype="<u4", count=3, offset=4))
        assert version in (1, 2), f"unknown version {version}"
        assert version == 2 or extra == 0, "v1 must declare extra=0"
        rec = np.dtype(
            [
                ("game_id", "<u4"),
                ("players", "u1"),
                ("actor", "u1"),
                ("round", "<u2"),
                ("margin", "<f4"),
                ("win_share", "<f4"),
                ("phi", "<f4", (dims,)),
                ("extra", "<f4", (extra,)),
            ]
        )
        raw = np.fromfile(fh, dtype=rec)
    return raw, dims, extra


def snake(name):
    """`HandCivil` -> `hand_civil`: the champion JSON's own key spelling for
    a `WeightKey` the `.keys` sidecar names in `Debug` form."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


class Folds:
    """Grouped 5-fold cross-products over `[X, 1, y]`.

    `M[f]` is the raw second-moment matrix of fold `f`'s rows; the training
    matrix for fold `f` is `total - M[f]`, so one pass over the data answers
    every subset question at every alpha.
    """

    def __init__(self, X, y, gid, k=5, seed=0, chunk=100_000):
        games = np.unique(gid)
        rng = np.random.default_rng(seed)
        rng.shuffle(games)
        which = {g: i % k for i, g in enumerate(games.tolist())}
        fold = np.array([which[g] for g in gid.tolist()], dtype=np.int8)
        self.k = k
        self.fold = fold
        self.p = X.shape[1]
        w = self.p + 2  # + intercept column + y
        self.M = np.zeros((k, w, w))
        for f in range(k):
            rows = np.flatnonzero(fold == f)
            for s in range(0, len(rows), chunk):
                idx = rows[s : s + chunk]
                B = np.empty((len(idx), w))
                B[:, : self.p] = X[idx]
                B[:, self.p] = 1.0
                B[:, self.p + 1] = y[idx]
                self.M[f] += B.T @ B
        self.total = self.M.sum(axis=0)
        # Pooled ss_tot: every row is held out exactly once, so the pooled
        # held-out set is the whole corpus and its mean is the global mean.
        n = self.total[self.p, self.p]
        sy = self.total[self.p, self.p + 1]
        syy = self.total[self.p + 1, self.p + 1]
        self.ss_tot = syy - sy * sy / n

    def r2(self, cols, alpha):
        """Pooled held-out R2 of ridge on `cols` (indices into X).

        Columns are standardized on each fold's own training rows -- the
        alpha then means the same thing for every subset regardless of the
        raw scale of the column being screened.
        """
        cols = np.asarray(cols, dtype=np.intp)
        p, iy = self.p, self.p + 1
        ss_res = 0.0
        for f in range(self.k):
            tr = self.total - self.M[f]
            n = tr[p, p]
            mu = tr[p, cols] / n
            ybar = tr[p, iy] / n
            cxx = tr[np.ix_(cols, cols)] - n * np.outer(mu, mu)
            cxy = tr[cols, iy] - n * mu * ybar
            var = np.maximum(np.diag(cxx) / n, 0.0)
            sd = np.sqrt(var)
            # A column with no variance in this fold's training rows carries
            # no information; sd=1 leaves it identically zero rather than
            # dividing by zero.
            sd[sd < 1e-12] = 1.0
            ztz = cxx / np.outer(sd, sd)
            ztz[np.diag_indices_from(ztz)] += alpha
            b = np.linalg.solve(ztz, cxy / sd)
            g = b / sd
            c = ybar - g @ mu
            te = self.M[f]
            n_te = te[p, p]
            sx = te[p, cols]
            sy = te[p, iy]
            sxy = te[cols, iy]
            sxx = te[np.ix_(cols, cols)]
            ss_res += (
                te[iy, iy]
                - 2 * c * sy
                - 2 * g @ sxy
                + n_te * c * c
                + 2 * c * (g @ sx)
                + g @ sxx @ g
            )
        return 1.0 - ss_res / self.ss_tot

    def gain_by_fold(self, cols, add, alpha):
        """Per-fold held-out R2 gain of `add` over `cols`, each fold scored
        against its own rows. The pooled figure is the headline; this is how
        a gain that is one lucky fold is told from one that is not."""
        out = []
        for f in range(self.k):
            keep = [f]
            sub = Folds.__new__(Folds)
            sub.k, sub.p, sub.M, sub.total = 1, self.p, self.M[keep], self.total
            iy = self.p + 1
            n = self.M[f][self.p, self.p]
            sy = self.M[f][self.p, iy]
            sub.ss_tot = self.M[f][iy, iy] - sy * sy / n
            out.append(sub.r2(list(cols) + list(add), alpha) - sub.r2(cols, alpha))
        return out

    def redundancy(self, cols, c, alpha=1e-6):
        """R2 of reconstructing column `c` from `cols` by least squares --
        how much of `c` the other columns already ARE.

        This is what makes a zero recovery in the positive control readable:
        a column the remaining features span exactly cannot raise held-out R2
        when it is handed back, however hard the evaluator leans on it. The
        screen measures UNIQUE signal, and this is the number that says so.
        """
        cols = np.asarray(list(cols), dtype=np.intp)
        tr = self.total
        n = tr[self.p, self.p]
        allc = np.append(cols, c)
        mu = tr[self.p, allc] / n
        cxx = tr[np.ix_(allc, allc)] - n * np.outer(mu, mu)
        sd = np.sqrt(np.maximum(np.diag(cxx) / n, 0.0))
        sd[sd < 1e-12] = 1.0
        r = cxx / np.outer(sd, sd) / n
        a, b = r[:-1, :-1].copy(), r[:-1, -1]
        a[np.diag_indices_from(a)] += alpha
        return float(b @ np.linalg.solve(a, b))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--label", default="margin")
    ap.add_argument("--alpha", type=float, default=1.0)
    ap.add_argument("--noise-cols", type=int, default=40)
    ap.add_argument(
        "--block-null-cols",
        type=int,
        default=151,
        help="a SECOND, disjoint gaussian pool used only for same-size block nulls. "
        "Kept out of the noise-floor pool on purpose: the floor is `max |gain|` over "
        "the pool, so widening the pool would raise the floor and the number would "
        "stop being comparable to the one already measured.",
    )
    ap.add_argument("--alphas", default="1,100,10000,1000000", help="alpha sweep for the summary")
    ap.add_argument("--fold-seed", type=int, default=0, help="which grouped 5-fold split to use")
    ap.add_argument(
        "--min-games",
        type=int,
        default=30,
        help="a candidate column that is non-zero in fewer than this many GAMES cannot be "
        "screened honestly: with a grouped split its whole contribution can land inside one "
        "fold, so its 'gain' is one game's residual. Such columns are still reported "
        "individually and flagged; they are additionally excluded from a SECOND, "
        "supplementary family total printed next to the raw one. Never replaces it.",
    )
    ap.add_argument("--champion", default=None, help="frozen champion JSON, for the weight column")
    args = ap.parse_args()

    raw, dims, extra = load(args.dump)
    keys = open(args.dump + ".keys").read().split()
    extra_keys = open(args.dump + ".extra_keys").read().split() if extra else []
    assert len(keys) == dims and len(extra_keys) == extra

    gid = raw["game_id"]
    y = raw[args.label].astype(np.float64)
    n_rows, n_games = len(raw), len(np.unique(gid))
    print(f"dump      {args.dump}")
    print(f"rows      {n_rows}   games {n_games}   phi dims {dims}   extra dims {extra}")
    print(f"label     {args.label}  (mean {y.mean():.3f}, sd {y.std():.3f})")
    print(f"alpha     {args.alpha}")

    # ---- assemble [phi | extras | synthetic controls] -------------------
    rng = np.random.default_rng(12345)
    noise = rng.standard_normal((n_rows, args.noise_cols))
    # An EXACT linear combination of three live phi columns. Ridge cannot
    # gain real information from it; whatever it scores is the screen's own
    # response to a redundant direction.
    lc_src = [keys.index(k) for k in ("Culture", "ScienceRate", "CivilActions") if k in keys]
    if len(lc_src) < 3:
        lc_src = [0, 1, 2]
    lincomb = (
        0.7 * raw["phi"][:, lc_src[0]].astype(np.float64)
        - 1.9 * raw["phi"][:, lc_src[1]].astype(np.float64)
        + 2.3 * raw["phi"][:, lc_src[2]].astype(np.float64)
    )
    # REAL candidate columns with their row order permuted: same marginal
    # distribution as a live candidate, no relationship to the label left.
    # One per candidate family, so no family's headline rests on a control
    # measured on a different family's column.
    want_perm = [
        "hand_playable_now_count",
        "hand_science_cost_total",
        "gran_wcomp_stage_total",
        "gran_board_obsolete_workers",
        "rel_proj_final_culture_gap",
        "rel_x_culrate_gap_late",
    ]
    perm_src = [extra_keys.index(k) for k in want_perm if k in extra_keys]
    perm = np.column_stack([rng.permutation(raw["extra"][:, i].astype(np.float64)) for i in perm_src])
    block_noise = rng.standard_normal((n_rows, args.block_null_cols))

    # How many GAMES each candidate column is ever non-zero in. A column
    # confined to a handful of games cannot be told apart from those games'
    # residual under a split that holds whole games out, so this is measured
    # before anything is fitted and printed next to every gain.
    _, gpos = np.unique(gid, return_inverse=True)
    n_games_total = int(gpos.max()) + 1
    extra_games = np.empty(extra, dtype=np.int64)
    for i in range(extra):
        nz = raw["extra"][:, i] != 0
        extra_games[i] = int((np.bincount(gpos[nz], minlength=n_games_total) > 0).sum())

    # Preallocated and filled column-block by column-block, then `raw` is
    # dropped: an hstack of these would hold two ~2 GB copies at once.
    names = list(keys) + list(extra_keys)
    names += [f"NULL_noise_{i}" for i in range(args.noise_cols)]
    names.append("NULL_lincomb_of_phi")
    names += [f"NULL_permuted:{extra_keys[i]}" for i in perm_src]
    names += [f"BLOCK_noise_{i}" for i in range(args.block_null_cols)]
    X = np.empty((n_rows, len(names)))
    X[:, :dims] = raw["phi"]
    X[:, dims : dims + extra] = raw["extra"]
    off = dims + extra
    X[:, off : off + args.noise_cols] = noise
    X[:, off + args.noise_cols] = lincomb
    X[:, off + args.noise_cols + 1 : off + args.noise_cols + 1 + len(perm_src)] = perm
    X[:, off + args.noise_cols + 1 + len(perm_src) :] = block_noise
    sd_raw = X[:, :dims].std(axis=0)
    del raw, noise, lincomb, perm, block_noise
    # Global centre and scale: a fixed, label-free linear transform that
    # keeps the cross-product matrices well conditioned. Per-fold
    # standardization inside `Folds.r2` is what actually defines the fit.
    X -= X.mean(axis=0)
    s = X.std(axis=0)
    s[s < 1e-12] = 1.0
    X /= s

    idx = {n: i for i, n in enumerate(names)}
    phi_cols = list(range(dims))
    extra_cols = [idx[k] for k in extra_keys]
    null_cols = [i for n, i in idx.items() if n.startswith("NULL_")]
    noise_only = [idx[f"NULL_noise_{i}"] for i in range(args.noise_cols)]
    block_pool = noise_only + [idx[f"BLOCK_noise_{i}"] for i in range(args.block_null_cols)]

    print("\nbuilding fold cross-products ...")
    F = Folds(X, y, gid, k=5, seed=args.fold_seed)
    print(f"5 folds by GAME (seed {args.fold_seed}); fold sizes {[int((F.fold == f).sum()) for f in range(5)]}")

    base = F.r2(phi_cols, args.alpha)
    print(f"\nBASE: ridge on phi ({dims} cols)      pooled held-out R2 {base:.6f}")

    # ---- (2) positive control -------------------------------------------
    # Pick columns with independent evidence of DECISION relevance: a
    # non-zero champion weight (the bot actually scores with them) times the
    # column's own spread (so the term moves the score at all).
    champ = {}
    if args.champion:
        import json

        champ = json.load(open(args.champion))["weights"]
    influence = np.array([abs(champ.get(snake(k), 0.0)) for k in keys]) * sd_raw
    order = np.argsort(-influence)
    probes = [int(i) for i in order[:8]]

    print("\n(2) POSITIVE CONTROL -- drop a live phi column, offer it back")
    print("    probes are the 8 phi columns with the largest |champion weight| x sd:")
    print("    the bot demonstrably scores with them, and the term actually moves its score.")
    print("    'spanned' = R2 of rebuilding the column from the other 173 (1.000 = the")
    print("    screen CANNOT see it, by construction, because nothing is missing when it goes).")
    print(f"    {'column':<26} {'|w|*sd':>9} {'spanned':>8} {'R2 base-1':>11} {'R2 +back':>10} {'gain':>10}")
    pos = []
    for c in probes:
        reduced = [i for i in phi_cols if i != c]
        r_red = F.r2(reduced, args.alpha)
        r_back = F.r2(reduced + [c], args.alpha)
        red = F.redundancy(reduced, c)
        pos.append((keys[c], influence[c], red, r_red, r_back, r_back - r_red))
        print(
            f"    {keys[c]:<26} {influence[c]:9.3f} {red:8.4f} {r_red:11.6f} "
            f"{r_back:10.6f} {r_back - r_red:+10.6f}"
        )
    best = max(pos, key=lambda t: t[5])
    print(f"    RECOVERY: best positive control is {best[0]} at {best[5]:+.6f} R2")

    # ---- (3) noise floor -------------------------------------------------
    print("\n(3) NEGATIVE CONTROLS -- gain of a column that cannot help")
    nulls = []
    for c in null_cols:
        g = F.r2(phi_cols + [c], args.alpha) - base
        nulls.append((names[c], g))
    named = dict(nulls)
    for n in [k for k in named if not k.startswith("NULL_noise_")]:
        print(f"    {n:<34} {named[n]:+.6f}")
    worst = sorted(((n, g) for n, g in nulls if n.startswith("NULL_noise_")), key=lambda t: -abs(t[1]))
    print(f"    {'worst of ' + str(args.noise_cols) + ' gaussian noise cols':<34} {worst[0][1]:+.6f}  ({worst[0][0]})")
    print(f"    {'median |gain| over those':<34} {np.median([abs(g) for _, g in worst]):+.6f}")
    floor = max(abs(g) for _, g in nulls)
    print(f"    ... {len(nulls)} null columns; NOISE FLOOR (max |gain|) = {floor:.6f} R2")

    # ---- (4) the candidate columns --------------------------------------
    # "spanned" is `redundancy`: how much of the candidate the 174 base
    # columns already reconstruct. Section 2 is what makes it necessary --
    # five of eight probes there recovered zero purely because they were
    # spanned, so a zero gain is only readable next to this number.
    print("\n(4) CANDIDATES -- gain over base phi, one column at a time")
    cands = []
    for k, c in zip(extra_keys, extra_cols):
        cands.append((k, F.r2(phi_cols + [c], args.alpha) - base, F.redundancy(phi_cols, c)))
    ng = dict(zip(extra_keys, extra_games.tolist()))
    rare = {k for k in extra_keys if ng[k] < args.min_games}
    print(f"    {'column':<34} {'gain':>10} {'x floor':>8} {'spanned':>8} {'games':>6}  per-fold gains")
    for k, g, sp in sorted(cands, key=lambda t: -t[1]):
        tag = "  RARE" if k in rare else ""
        if ng[k] == 0:
            print(f"    {k:<34} {g:+.6f} {'':>8} {sp:8.4f} {0:6d}  CONSTANT IN THIS DUMP: never non-zero")
        elif abs(g) > floor:
            per = F.gain_by_fold(phi_cols, [idx[k]], args.alpha)
            spread = "  ".join(f"{v:+.5f}" for v in per)
            sign = "all +" if all(v > 0 for v in per) else ("all -" if all(v < 0 for v in per) else "MIXED")
            print(f"    {k:<34} {g:+.6f} {g / floor:7.1f}x {sp:8.4f} {ng[k]:6d}  {spread}  [{sign}]{tag}")
        else:
            print(f"    {k:<34} {g:+.6f} {'':>8} {sp:8.4f} {ng[k]:6d}  (< noise floor: NULL){tag}")
    print(f"    {len(rare)} column(s) non-zero in fewer than {args.min_games} games, flagged RARE/CONSTANT above.")

    # Families are keyed off the COLUMN-NAME PREFIX rather than a slice of
    # `extra_keys`, so adding a column to `feature_screen.rs` cannot silently
    # shift another family's membership.
    def pref(*p):
        return [idx[k] for k in extra_keys if k.startswith(p)]

    hand_cols = [idx[k] for k in extra_keys if not k.startswith(("gran_", "rel_", "ctrl_a_", "ctrl_b_"))]
    fam_a = pref("gran_", "ctrl_a_")
    fam_b = pref("rel_", "ctrl_b_")
    groups = {
        "HAND COMPOSITION (all)": hand_cols,
        "  (A) type families": [idx[k] for k in extra_keys[0:7]],
        "  (B) ages": [idx[k] for k in extra_keys[7:14]],
        "  (C) playable now": [idx[k] for k in extra_keys[14:19]],
        "  (D) cost mass": [idx[k] for k in extra_keys[19:23]],
        "  (E) military hand": [idx[k] for k in extra_keys[23:30]],
        "  (F) hidden counts": [idx[k] for k in extra_keys[30:32]],
        "CARD GRANULARITY (all)": fam_a,
        "  gov identity one-hot": pref("gran_gov_"),
        "  special-tech one-hot": pref("gran_spec_"),
        "  unit level by type": pref("gran_best_"),
        "  wonder completed one-hot": [idx[k] for k in extra_keys if k.startswith("gran_wcomp_") and k != "gran_wcomp_stage_total"],
        "  wonder in-progress one-hot": [idx[k] for k in extra_keys if k.startswith("gran_wbuild_") and not k.startswith("gran_wbuild_stage") and k != "gran_wbuild_max_stage" and k != "gran_wbuild_num_stages"],
        "  leader one-hot": [idx[k] for k in extra_keys if k.startswith("gran_leader_") and k != "gran_leader_age"],
        "  obsolescence + cost/benefit": pref("gran_board_", "gran_hand_"),
        "OPPONENT-RELATIVE (all)": fam_b,
        "  hinged halves": pref("rel_culrate", "rel_scirate", "rel_culture", "rel_strength", "rel_tech", "rel_scistock"),
        "  sign x magnitude buckets": pref("rel_bkt_"),
        "  gap x lateness / round": pref("rel_x_"),
        "  projection + shares": pref("rel_proj_", "rel_trail_", "rel_share_"),
        "  gap-conditional card value": pref("rel_cond_"),
        "  tempo (CA / mil hand size)": pref("rel_ca_", "rel_milhand_"),
        "ALL CANDIDATE COLUMNS": extra_cols,
        "ALL minus every ctrl_": [c for k, c in zip(extra_keys, extra_cols) if not k.startswith("ctrl_")],
    }
    # `keep` is the same block with every RARE column removed. Printed
    # alongside, never instead of, the raw figure.
    rare_cols = {idx[k] for k in rare}
    print("\n    FAMILY / SUBSET gains over base phi (whole block added at once)")
    print(f"    {'block':<34} {'raw':>10} {'':>6}  {'minus RARE':>11} {'':>6}")
    for name, cols in groups.items():
        keep = [c for c in cols if c not in rare_cols]
        raw_g = F.r2(phi_cols + list(cols), args.alpha) - base
        line = f"    {name:<34} {raw_g:+.6f} ({len(cols):>3})"
        if len(keep) != len(cols):
            line += f"   {F.r2(phi_cols + keep, args.alpha) - base:+11.6f} ({len(keep):>3})"
        print(line)

    # Same-size nulls: a block of N pure-noise columns for each N that a
    # family actually has, so a family total is compared against a block
    # figure and not a one-column floor.
    print("\n    SAME-SIZE NULLS -- N gaussian columns added at once")
    for n in sorted({len(c) for c in groups.values()}):
        if n > len(block_pool):
            print(f"    {'NULL: ' + str(n) + ' noise columns':<34} (pool too small: {len(block_pool)})")
            continue
        print(f"    {'NULL: ' + str(n) + ' noise columns':<34} {F.r2(phi_cols + block_pool[:n], args.alpha) - base:+.6f}")

    # ---- (5) alpha sweep -------------------------------------------------
    # A gain that only exists at one ridge penalty is a property of the
    # penalty, not of the column.
    print("\n(5) ALPHA SWEEP -- does any of this depend on the ridge penalty?")
    pc = probes[int(np.argmax([p[5] for p in pos]))]
    pc_reduced = [i for i in phi_cols if i != pc]
    top_names = [t[0] for t in sorted(cands, key=lambda t: -t[1])[:3]]
    top = [idx[k] for k in top_names]
    hdr = f"    {'alpha':>7} {'base R2':>10} {'poscontrol':>11} {'floor':>10} {'hand':>10} {'granul':>10} {'relative':>10}"
    hdr += "".join(f" {n[:14]:>15}" for n in top_names)
    print(hdr)
    for a in (float(v) for v in args.alphas.split(",")):
        b = F.r2(phi_cols, a)
        pcg = F.r2(pc_reduced + [pc], a) - F.r2(pc_reduced, a)
        fl = max(abs(F.r2(phi_cols + [c], a) - b) for c in null_cols)
        row = f"    {a:7.1f} {b:10.6f} {pcg:+11.6f} {fl:10.6f}"
        for cols in (hand_cols, fam_a, fam_b):
            row += f" {F.r2(phi_cols + list(cols), a) - b:+10.6f}"
        row += "".join(f" {F.r2(phi_cols + [c], a) - b:+15.6f}" for c in top)
        print(row)
    print(f"    (positive control column: {keys[pc]})")


if __name__ == "__main__":
    main()
