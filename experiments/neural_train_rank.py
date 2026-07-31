"""Train the value net with a COMBINED value + pairwise-ranking objective (GPU).

Loss = value_mse( v(Xv), yv/scale )  +  lambda * BT( v(Xa) - v(Xb) )
where BT = softplus(-(v(chosen) - v(rejected))) is the Bradley-Terry / logistic
ranking loss that pushes the chosen sibling above each rejected one -- the exact
signal the 1-ply argmax consumes, which plain MC regression starves
(docs/BOT_ARCHITECTURE.md 3b, docs/NEURAL_EVAL.md).

Val is a random ROW split by default and every reported number is only as
meaningful as the LABELS in the data.  Read docs/NEURAL_LOOP_NULL.md before
trusting `val_pair_acc`: when the "chosen" sibling was produced by the model
being warm-started from, pair accuracy is an agreement-with-incumbent meter,
not a quality metric, and selecting on it selects for doing nothing.  That is
why this script now prints **epoch 0** and a `VACUITY` line: if the untrained
warm-start already orders the training pairs correctly, the target is a fixed
point and the run is a no-op.  Play strength head-to-head under the search that
will be deployed (neural_eval.py) is the only promotion criterion.

Usage:
    python neural_train_rank.py --data teacherdata/*.npz --epochs 25 \
        --lam 1.0 --out checkpoints/value2p_rank.pt
"""
from __future__ import annotations

import argparse
import glob
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

from engine.bots.neural_net import ValueNet, save_checkpoint, MARGIN_NORM


def load(patterns):
    files = []
    for pat in patterns:
        hits = glob.glob(pat)
        files.extend(hits if hits else ([pat] if os.path.exists(pat) else []))
    files = sorted(set(files))
    if not files:
        raise SystemExit(f"no shards match {patterns!r}")
    return files


def read(files):
    Xa, Xb, Xv, yv = [], [], [], []
    for f in files:
        d = np.load(f)
        Xa.append(d["Xa"].astype(np.float32))
        Xb.append(d["Xb"].astype(np.float32))
        Xv.append(d["Xv"].astype(np.float32))
        yv.append(d["yv"].astype(np.float32))
    return (np.concatenate(Xa), np.concatenate(Xb),
            np.concatenate(Xv), np.concatenate(yv))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True, nargs="+")
    ap.add_argument("--epochs", type=int, default=25)
    ap.add_argument("--batch", type=int, default=4096)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--wd", type=float, default=2e-4)
    ap.add_argument("--hidden", type=int, default=256)
    ap.add_argument("--blocks", type=int, default=3)
    ap.add_argument("--dropout", type=float, default=0.15)
    ap.add_argument("--lam", type=float, default=1.0, help="ranking weight")
    ap.add_argument("--vweight", type=float, default=1.0,
                    help="value-loss weight; raise it to pin the output SCALE "
                         "(the BT ranking loss is scale-hungry and decalibrates "
                         "the value head at vweight=1 -- val MAE ballooned to 84 "
                         "in Stage 1b). See docs/NEURAL_EVAL.md.")
    ap.add_argument("--init", default=None,
                    help="warm-start checkpoint (for the self-play loop)")
    ap.add_argument("--select", default="last",
                    choices=("pair", "mae", "combo", "last"),
                    help="best-checkpoint criterion. DEFAULT IS `last`: the "
                         "41-hour null (docs/NEURAL_LOOP_NULL.md 3.2) selected "
                         "on `pair` against a validation set whose labels the "
                         "INCUMBENT had produced, so `pair` was an "
                         "agreement-with-incumbent meter and selecting on it "
                         "picked epoch 1 every time -- a regulariser toward "
                         "doing nothing. The gate decides now, not the loss.")
    ap.add_argument("--val-frac", type=float, default=0.15)
    ap.add_argument("--val-split", default="rows", choices=("rows", "shards"),
                    help="`rows` (default): a random row-level split, so val "
                         "is the SAME distribution as train. `shards`: the old "
                         "behaviour -- the alphabetically first 15% of files, "
                         "which silently made val a single data SOURCE (all "
                         "self-play, none of the anchor) and made every "
                         "reported number a distribution-shift artefact.")
    ap.add_argument("--vacuity-warn", type=float, default=0.95,
                    help="fail loudly if the UNTRAINED warm-start already "
                         "orders this fraction of the training pairs "
                         "correctly: the target is then a fixed point of the "
                         "model and the run cannot learn anything "
                         "(docs/NEURAL_LOOP_NULL.md 3.1 measured 0.9764)")
    ap.add_argument("--out", default="checkpoints/value_rank.pt")
    ap.add_argument("--device", default="cuda")
    args = ap.parse_args()

    device = args.device if torch.cuda.is_available() else "cpu"
    print(f"device={device} cuda={torch.cuda.is_available()}", flush=True)
    if device.startswith("cuda"):
        print("gpu:", torch.cuda.get_device_name(0), flush=True)

    files = load(args.data)
    if args.val_split == "shards":
        nval = max(1, int(round(len(files) * args.val_frac)))
        vfiles, tfiles = files[:nval], (files[nval:] or files)
        Xa, Xb, Xv, yv = read(tfiles)
        Xa_v, Xb_v, Xv_v, yv_v = read(vfiles)
        print(f"{len(files)} shards -> {len(tfiles)} train / {len(vfiles)} val "
              f"(SHARD split: val may be a single data source -- see "
              f"docs/NEURAL_LOOP_NULL.md 3.2)", flush=True)
    else:
        aA, aB, aV, aY = read(files)
        rs = np.random.RandomState(12345)
        pi = rs.permutation(len(aA))
        pv = rs.permutation(len(aV))
        ka = max(1, int(round(len(aA) * args.val_frac)))
        kv = max(1, int(round(len(aV) * args.val_frac)))
        Xa, Xb = aA[pi[ka:]], aB[pi[ka:]]
        Xa_v, Xb_v = aA[pi[:ka]], aB[pi[:ka]]
        Xv, yv = aV[pv[kv:]], aY[pv[kv:]]
        Xv_v, yv_v = aV[pv[:kv]], aY[pv[:kv]]
        print(f"{len(files)} shards -> random ROW split {args.val_frac:.2f}",
              flush=True)
    print(f"train pairs {len(Xa)}  val pairs {len(Xa_v)}  "
          f"train vals {len(Xv)}  dim {Xa.shape[1]}", flush=True)

    net = ValueNet(Xa.shape[1], args.hidden, args.blocks, args.dropout).to(device)
    if args.init and os.path.exists(args.init):
        obj = torch.load(args.init, map_location=device, weights_only=False)
        net.load_state_dict(obj["state_dict"])
        print(f"warm-started from {args.init}", flush=True)
    opt = torch.optim.AdamW(net.parameters(), lr=args.lr, weight_decay=args.wd)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, args.epochs)

    Xa_t, Xb_t = torch.tensor(Xa), torch.tensor(Xb)
    Xv_t, yv_t = torch.tensor(Xv), torch.tensor(yv / MARGIN_NORM)
    Xa_vt = torch.tensor(Xa_v, device=device)
    Xb_vt = torch.tensor(Xb_v, device=device)
    Xv_vt = torch.tensor(Xv_v, device=device)
    yv_vs = yv_v / MARGIN_NORM

    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)

    def evaluate_val():
        net.eval()
        with torch.no_grad():
            va = torch.cat([net(Xa_vt[i:i + 8192])
                            for i in range(0, len(Xa_vt), 8192)])
            vb = torch.cat([net(Xb_vt[i:i + 8192])
                            for i in range(0, len(Xb_vt), 8192)])
            pa = (va > vb).float().mean().item()
            vp = torch.cat([net(Xv_vt[i:i + 8192]).cpu()
                            for i in range(0, len(Xv_vt), 8192)]).numpy()
        return pa, float(np.abs(vp - yv_vs).mean() * MARGIN_NORM)

    # --- epoch 0: the vacuity check ----------------------------------------
    # docs/NEURAL_LOOP_NULL.md 3.1: the 41-hour null trained on ranking pairs
    # whose "chosen" label was the warm-start's OWN argmax, so the untrained
    # net already ordered 97.6% of them correctly and the loss had nothing to
    # teach.  Nobody looked, because epoch 0 was never printed.  It is printed
    # now, and a target that is already satisfied is a loud failure, not a
    # footnote.
    pa0, mae0 = evaluate_val()
    print(f"epoch   0  (warm start, UNTRAINED)          "
          f"val_pair_acc {pa0:.4f}  val_mae {mae0:.1f}", flush=True)
    print(f"VACUITY pair_acc_at_epoch0={pa0:.4f} "
          f"threshold={args.vacuity_warn:.2f}", flush=True)
    if pa0 >= args.vacuity_warn:
        print("*** VACUOUS TARGET: the warm-start already satisfies "
              f"{pa0:.1%} of the ranking pairs. The label is a fixed point of "
              "the model being trained -- gradient descent can only inflate "
              "margins it already holds. This is the exact defect that cost "
              "41 hours and 0 promotions (docs/NEURAL_LOOP_NULL.md). Do not "
              "trust anything downstream of this run. ***", flush=True)

    npairs = len(Xa_t)
    nval_rows = len(Xv_t)
    best_score, best_ep, best_pa_at, best_mae_at = -1e9, 0, 0.0, 0.0
    for ep in range(args.epochs):
        net.train()
        perm = torch.randperm(npairs)
        vperm = torch.randperm(nval_rows)
        tot_r = tot_v = 0.0
        nb = 0
        for i in range(0, npairs, args.batch):
            ix = perm[i:i + args.batch]
            xa = Xa_t[ix].to(device, non_blocking=True)
            xb = Xb_t[ix].to(device, non_blocking=True)
            # a value minibatch (cycled) alongside each ranking minibatch
            vix = vperm[(i // args.batch * args.batch) % nval_rows:][:args.batch]
            if len(vix) == 0:
                vix = vperm[:args.batch]
            xv = Xv_t[vix].to(device, non_blocking=True)
            yv_b = yv_t[vix].to(device, non_blocking=True)
            opt.zero_grad()
            va = net(xa)
            vb = net(xb)
            rank = F.softplus(-(va - vb)).mean()
            vpred = net(xv)
            vloss = F.smooth_l1_loss(vpred, yv_b)
            loss = args.vweight * vloss + args.lam * rank
            loss.backward()
            opt.step()
            tot_r += rank.item()
            tot_v += vloss.item()
            nb += 1
        sched.step()
        pair_acc, val_mae = evaluate_val()
        if args.select == "last":
            score = float(ep)          # monotone: always keep the last epoch
        elif args.select == "pair":
            score = pair_acc
        elif args.select == "mae":
            score = -val_mae
        else:  # combo: keep ranking high AND value calibrated
            score = pair_acc - val_mae / 300.0
        best = score > best_score
        print(f"epoch {ep+1:3d}  rank {tot_r/nb:.4f}  vloss {tot_v/nb:.4f}  "
              f"val_pair_acc {pair_acc:.4f}  val_mae {val_mae:.1f}"
              f"{'  *best' if best else ''}", flush=True)
        if best:
            best_score, best_ep = score, ep + 1
            best_pa_at, best_mae_at = pair_acc, val_mae
            save_checkpoint(args.out, net, meta={
                "val_pair_acc": pair_acc, "val_mae_culture": val_mae,
                "epoch": ep + 1, "lam": args.lam, "vweight": args.vweight,
                "hidden": args.hidden, "blocks": args.blocks})
    print(f"best epoch {best_ep}: pair_acc {best_pa_at:.4f} mae {best_mae_at:.1f}"
          f"; saved {args.out}", flush=True)


if __name__ == "__main__":
    main()
