#!/usr/bin/env python3
"""Kill-switch for the nonlinear-leaf-eval question.

Reads a `phidump` file (see `rust/src/bin/phidump.rs` for the format) and asks
one thing: does a nonlinear function of the champion's own feature vector
predict the game outcome better than the BEST LINEAR function of the same
vector?

The control is deliberately the best linear fit, not the champion's own dot
product. The champion was never fit to predict an outcome, so beating it
proves nothing about linearity -- only a gap over ridge does.

Games are split, not rows: every decision from one game shares a label, so a
row-wise split leaks the answer across the fold.
"""

import sys
import numpy as np
from sklearn.linear_model import Ridge
from sklearn.neural_network import MLPRegressor
from sklearn.preprocessing import StandardScaler
from sklearn.dummy import DummyRegressor

HEADER = 16


def load(path):
    with open(path, "rb") as fh:
        head = fh.read(HEADER)
        assert head[:4] == b"TPHI", f"bad magic {head[:4]!r}"
        version, dims = np.frombuffer(head, dtype="<u4", count=3, offset=4)[:2]
        assert version == 1, f"unknown version {version}"
        dims = int(dims)
        rec = np.dtype(
            [
                ("game_id", "<u4"),
                ("players", "u1"),
                ("actor", "u1"),
                ("round", "<u2"),
                ("margin", "<f4"),
                ("win_share", "<f4"),
                ("phi", "<f4", (dims,)),
            ]
        )
        raw = np.fromfile(fh, dtype=rec)
    return raw, dims


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/phi_2p.bin"
    label = sys.argv[2] if len(sys.argv) > 2 else "margin"
    raw, dims = load(path)
    print(f"records {len(raw)}  dims {dims}  games {len(np.unique(raw['game_id']))}")

    X = raw["phi"].astype(np.float64)
    y = raw[label].astype(np.float64)
    gid = raw["game_id"]

    games = np.unique(gid)
    rng = np.random.default_rng(0)
    rng.shuffle(games)
    cut = int(0.8 * len(games))
    train_games = set(games[:cut].tolist())
    tr = np.array([g in train_games for g in gid])
    te = ~tr
    print(f"train rows {tr.sum()} ({cut} games)   held-out rows {te.sum()} ({len(games)-cut} games)")

    scaler = StandardScaler().fit(X[tr])
    Xtr, Xte = scaler.transform(X[tr]), scaler.transform(X[te])
    ytr, yte = y[tr], y[te]

    def report(name, model):
        model.fit(Xtr, ytr)
        pred = model.predict(Xte)
        rmse = float(np.sqrt(np.mean((pred - yte) ** 2)))
        ss_res = float(np.sum((pred - yte) ** 2))
        ss_tot = float(np.sum((yte - yte.mean()) ** 2))
        r2 = 1 - ss_res / ss_tot
        print(f"{name:<28} held-out RMSE {rmse:8.3f}   R2 {r2:7.4f}")
        return r2

    print()
    base = report("constant (mean)", DummyRegressor(strategy="mean"))
    lin = report("ridge (best LINEAR on phi)", Ridge(alpha=1.0))
    mlp = report(
        "MLP 64-64 (nonlinear)",
        MLPRegressor(
            hidden_layer_sizes=(64, 64),
            max_iter=60,
            early_stopping=True,
            n_iter_no_change=5,
            random_state=0,
        ),
    )
    print()
    print(f"nonlinear gain over best linear: {mlp - lin:+.4f} R2  ({(mlp-lin)/max(lin,1e-9)*100:+.1f}% relative)")
    print(f"linear gain over constant:       {lin - base:+.4f} R2")


if __name__ == "__main__":
    main()
