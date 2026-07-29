"""Diagnostic: is the self-play ranking label a fixed point of the net itself?

Loads the incumbent (best.pt) and, WITHOUT training, measures pair accuracy
separately on (a) the on-policy self-play shards (iterdata/*) whose "chosen"
label is the net's OWN argmax, and (b) the BookBot anchor shards (rankdata/*)
whose "chosen" label comes from an external teacher.

If (a) is ~1.0 the ranking loss on self-play data carries zero information:
the net already satisfies it, and gradient descent can only sharpen margins.
"""
import glob
import sys
import numpy as np
import torch

sys.path.insert(0, ".")
from engine.bots.neural_net import load_checkpoint, MARGIN_SCALE

ck = sys.argv[1] if len(sys.argv) > 1 else "checkpoints/best.pt"
dev = "cuda" if torch.cuda.is_available() else "cpu"
net, obj = load_checkpoint(ck, dev)
net.eval()
print("ckpt", ck, "meta", obj.get("meta"))


def score(files, label):
    na = nc = 0
    marg = []
    yvs = []
    vpred = []
    for f in files:
        d = np.load(f)
        Xa = d["Xa"].astype(np.float32)
        Xb = d["Xb"].astype(np.float32)
        Xv = d["Xv"].astype(np.float32)
        yv = d["yv"].astype(np.float32)
        with torch.no_grad():
            for i in range(0, len(Xa), 8192):
                a = net(torch.tensor(Xa[i:i + 8192], device=dev))
                b = net(torch.tensor(Xb[i:i + 8192], device=dev))
                nc += int((a > b).sum().item())
                na += len(a)
                marg.append((a - b).cpu().numpy())
            for i in range(0, len(Xv), 8192):
                vpred.append(net(torch.tensor(Xv[i:i + 8192], device=dev)).cpu().numpy())
        yvs.append(yv)
    marg = np.concatenate(marg) * MARGIN_SCALE
    yvs = np.concatenate(yvs)
    vpred = np.concatenate(vpred) * MARGIN_SCALE
    mae = float(np.abs(vpred - yvs).mean())
    print(f"\n== {label}: {len(files)} shards, {na} pairs, {len(yvs)} value rows")
    print(f"   pair_acc (incumbent, UNTRAINED) = {nc/na:.4f}")
    print(f"   chosen-minus-rejected margin: mean {marg.mean():+.2f} "
          f"median {np.median(marg):+.2f} culture; frac |m|<0.5: "
          f"{float((np.abs(marg) < 0.5).mean()):.3f}")
    print(f"   value target yv: mean {yvs.mean():+.1f} sd {yvs.std():.1f} "
          f"|yv| mean {np.abs(yvs).mean():.1f}")
    print(f"   value pred: mean {vpred.mean():+.1f} sd {vpred.std():.1f}  MAE {mae:.1f}")
    print(f"   MAE of the trivial predictor v=0: {float(np.abs(yvs).mean()):.1f}")
    return na


self_files = sorted(glob.glob("iterdata/it73_w*.npz")) + sorted(glob.glob("iterdata/it72_w*.npz"))
book_files = sorted(glob.glob("rankdata/rk_*.npz"))
score(self_files, "SELF-PLAY (label = net's own argmax)")
score(book_files, "BOOKBOT ANCHOR (label = external teacher)")
