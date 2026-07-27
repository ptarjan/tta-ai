"""Train the value net with a COMBINED value + pairwise-ranking objective (GPU).

Loss = value_mse( v(Xv), yv/scale )  +  lambda * BT( v(Xa) - v(Xb) )
where BT = softplus(-(v(chosen) - v(rejected))) is the Bradley-Terry / logistic
ranking loss that pushes the chosen sibling above each rejected one -- the exact
signal the 1-ply argmax consumes, which plain MC regression starves
(docs/BOT_ARCHITECTURE.md 3b, docs/NEURAL_EVAL.md).

Splits shards by shard for val.  Reports val VALUE mae (culture) and, the metric
that matters here, val PAIR ACCURACY: the fraction of held-out (chosen,rejected)
pairs the net orders correctly.  Best-val-pair-accuracy checkpoint is kept.
Play strength is still the real test (neural_eval.py).

Usage:
    python neural_train_rank.py --data rankdata/*.npz --epochs 25 \
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

from engine.bots.neural_net import ValueNet, save_checkpoint, MARGIN_SCALE


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
    ap.add_argument("--select", default="combo",
                    choices=("pair", "mae", "combo"),
                    help="best-checkpoint criterion: pair acc, value MAE, or a "
                         "combo (pair_acc - mae/300) that keeps both healthy")
    ap.add_argument("--val-frac", type=float, default=0.15)
    ap.add_argument("--out", default="checkpoints/value_rank.pt")
    ap.add_argument("--device", default="cuda")
    args = ap.parse_args()

    device = args.device if torch.cuda.is_available() else "cpu"
    print(f"device={device} cuda={torch.cuda.is_available()}", flush=True)
    if device.startswith("cuda"):
        print("gpu:", torch.cuda.get_device_name(0), flush=True)

    files = load(args.data)
    nval = max(1, int(round(len(files) * args.val_frac)))
    vfiles, tfiles = files[:nval], (files[nval:] or files)
    Xa, Xb, Xv, yv = read(tfiles)
    Xa_v, Xb_v, Xv_v, yv_v = read(vfiles)
    print(f"{len(files)} shards -> {len(tfiles)} train / {len(vfiles)} val",
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
    Xv_t, yv_t = torch.tensor(Xv), torch.tensor(yv / MARGIN_SCALE)
    Xa_vt = torch.tensor(Xa_v, device=device)
    Xb_vt = torch.tensor(Xb_v, device=device)
    Xv_vt = torch.tensor(Xv_v, device=device)
    yv_vs = yv_v / MARGIN_SCALE

    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)
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
        # validation
        net.eval()
        with torch.no_grad():
            va = []
            vb = []
            for i in range(0, len(Xa_vt), 8192):
                va.append(net(Xa_vt[i:i + 8192]))
                vb.append(net(Xb_vt[i:i + 8192]))
            va = torch.cat(va)
            vb = torch.cat(vb)
            pair_acc = (va > vb).float().mean().item()
            vp = []
            for i in range(0, len(Xv_vt), 8192):
                vp.append(net(Xv_vt[i:i + 8192]).cpu())
            vp = torch.cat(vp).numpy()
        val_mae = float(np.abs(vp - yv_vs).mean() * MARGIN_SCALE)
        if args.select == "pair":
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
