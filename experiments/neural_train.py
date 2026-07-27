"""Train the value net on self-play shards (GPU).  Runs on the desktop.

Splits shards into train/val BY SHARD (never by row) so correlated within-game
rows cannot straddle the split and leak.  Reports val loss and, more
importantly, val pairwise-ranking accuracy -- the fraction of same-game seat
pairs the net orders correctly -- because that is the closest cheap proxy to
what the 1-ply bot actually needs (rank sibling states), per
docs/BOT_ARCHITECTURE.md 2.3b.  Play strength is still measured head-to-head
by neural_eval.py; training loss is never the deliverable.

Usage:
    python neural_train.py --data 'data/sp2p.*.npz' --epochs 20 \
        --out checkpoints/value2p.pt
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

from engine.bots.neural_net import ValueNet, save_checkpoint, MARGIN_SCALE


def load_shards(patterns):
    files = []
    for pat in patterns:
        hits = glob.glob(pat)
        files.extend(hits if hits else ([pat] if os.path.exists(pat) else []))
    files = sorted(set(files))
    if not files:
        raise SystemExit(f"no shards match {patterns!r}")
    return files


def read(files):
    Xs, Ys = [], []
    for f in files:
        d = np.load(f)
        Xs.append(d["X"].astype(np.float32))
        Ys.append(d["y"].astype(np.float32))
    return np.concatenate(Xs), np.concatenate(Ys)


def ranking_accuracy(pred, y, groups=None, n=200000):
    """Fraction of random index pairs the prediction orders like the label.
    (A cheap global proxy for the per-game sibling ranking the bot needs.)"""
    import numpy as _np
    m = len(pred)
    if m < 2:
        return float("nan")
    rng = _np.random.default_rng(0)
    a = rng.integers(0, m, size=n)
    b = rng.integers(0, m, size=n)
    keep = y[a] != y[b]
    a, b = a[keep], b[keep]
    correct = ((pred[a] > pred[b]) == (y[a] > y[b])).mean()
    return float(correct)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True, nargs="+",
                    help="one or more *.npz shards or globs")
    ap.add_argument("--epochs", type=int, default=20)
    ap.add_argument("--batch", type=int, default=4096)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--wd", type=float, default=1e-4)
    ap.add_argument("--hidden", type=int, default=256)
    ap.add_argument("--blocks", type=int, default=3)
    ap.add_argument("--dropout", type=float, default=0.1)
    ap.add_argument("--val-frac", type=float, default=0.15)
    ap.add_argument("--huber", type=float, default=1.0,
                    help="Huber delta (0 = plain MSE)")
    ap.add_argument("--out", default="checkpoints/value.pt")
    ap.add_argument("--device", default="cuda")
    args = ap.parse_args()

    device = args.device if torch.cuda.is_available() else "cpu"
    print(f"device={device}  torch={torch.__version__}  "
          f"cuda={torch.cuda.is_available()}", flush=True)
    if device.startswith("cuda"):
        print("gpu:", torch.cuda.get_device_name(0), flush=True)

    files = load_shards(args.data)
    nval = max(1, int(round(len(files) * args.val_frac)))
    val_files = files[:nval]
    train_files = files[nval:] or files      # never empty
    print(f"{len(files)} shards -> {len(train_files)} train / "
          f"{len(val_files)} val (split by shard)", flush=True)

    Xtr, Ytr = read(train_files)
    Xva, Yva = read(val_files)
    print(f"train rows {len(Xtr)}  val rows {len(Xva)}  dim {Xtr.shape[1]}",
          flush=True)

    in_dim = Xtr.shape[1]
    net = ValueNet(in_dim, args.hidden, args.blocks, args.dropout).to(device)
    opt = torch.optim.AdamW(net.parameters(), lr=args.lr, weight_decay=args.wd)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, args.epochs)
    lossfn = (nn.SmoothL1Loss(beta=args.huber) if args.huber > 0
              else nn.MSELoss())

    # scaled targets
    ytr = torch.tensor(Ytr / MARGIN_SCALE)
    yva_scaled = Yva / MARGIN_SCALE
    Xtr_t = torch.tensor(Xtr)
    Xva_t = torch.tensor(Xva, device=device)

    os.makedirs(os.path.dirname(os.path.abspath(args.out)) or ".", exist_ok=True)
    best_racc = -1.0
    best_ep = 0

    n = len(Xtr_t)
    for ep in range(args.epochs):
        net.train()
        perm = torch.randperm(n)
        tot = 0.0
        for i in range(0, n, args.batch):
            ix = perm[i:i + args.batch]
            xb = Xtr_t[ix].to(device, non_blocking=True)
            yb = ytr[ix].to(device, non_blocking=True)
            opt.zero_grad()
            out = net(xb)
            loss = lossfn(out, yb)
            loss.backward()
            opt.step()
            tot += loss.item() * len(ix)
        sched.step()
        # validation
        net.eval()
        with torch.no_grad():
            preds = []
            for i in range(0, len(Xva_t), 8192):
                preds.append(net(Xva_t[i:i + 8192]).cpu())
            pv = torch.cat(preds).numpy()
        val_mse = float(((pv - yva_scaled) ** 2).mean())
        val_mae_culture = float(np.abs(pv - yva_scaled).mean() * MARGIN_SCALE)
        racc = ranking_accuracy(pv, yva_scaled)
        best = racc > best_racc
        print(f"epoch {ep + 1:3d}  train_loss {tot / n:.4f}  "
              f"val_mse {val_mse:.4f}  val_mae {val_mae_culture:.1f} culture  "
              f"val_rank_acc {racc:.4f}{'  *best' if best else ''}", flush=True)
        # early-stopping on val ranking accuracy: the checkpoint we keep is the
        # BEST val epoch, not the last (the last overfits -- see the epoch-30
        # regression in docs/NEURAL_EVAL.md).
        if best:
            best_racc, best_ep = racc, ep + 1
            save_checkpoint(args.out, net, meta={
                "train_rows": len(Xtr), "val_rows": len(Xva),
                "val_rank_acc": racc, "val_mae_culture": val_mae_culture,
                "epoch": ep + 1, "hidden": args.hidden, "blocks": args.blocks,
            })

    print(f"best val_rank_acc {best_racc:.4f} at epoch {best_ep}; "
          f"saved {args.out}", flush=True)


if __name__ == "__main__":
    main()
