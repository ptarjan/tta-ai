"""The value network and its inference wrapper (torch-guarded).

`import torch` is deferred so that importing this module fails cleanly on a
machine without torch (the Mac): callers must catch ImportError or check
`HAVE_TORCH` first.  The engine's own tests and `tools/gate.sh` never import
this module; only the neural tests (which skip when torch is absent), the
training/self-play scripts, and `NeuralBot` do.

Target
------
The net predicts the eventual **final-culture margin** of player ``idx``
(``my final culture - the best rival's final culture``), scaled by
:data:`MARGIN_NORM`.  Margin, not win/loss, because it is a dense label that
exists on every state and separates "lost by 8" from "lost by 90"; the greedy
policy that consumes it only needs the ARGMAX over sibling states, and margin
preserves that ordering.  docs/NEURAL_EVAL.md records the caveat from
docs/BOT_ARCHITECTURE.md 3b: a better Monte-Carlo predictor can still be a
worse greedy policy, so play strength is measured head-to-head, never by loss.
"""
from __future__ import annotations

try:
    import torch
    import torch.nn as nn
    HAVE_TORCH = True
except ImportError:                     # pragma: no cover - Mac has no torch
    torch = None
    nn = object
    HAVE_TORCH = False

#: NUMERICAL GUARD, and it is NOT `hillclimb_pool.MARGIN_SCALE`.  It used to
#: be called `MARGIN_SCALE`, which read as the same knob and is not: that one
#: is the tanh SQUASH width of the league objective, a modelling choice about
#: how much a decisive game is worth (120.0, and changing it changes what the
#: climb maximises).  This one is a plain LINEAR NORMALISER on the value net's
#: regression target -- divide by it to train, multiply by it to read the
#: prediction back in culture points -- so it cancels exactly and any value of
#: the right order does the same job.  Final margins are ~[-250, 250]; /100
#: puts the target near unit scale without clipping.  The checkpoint key stays
#: spelled `margin_scale` because it is serialised data in existing files.
MARGIN_NORM = 100.0


if HAVE_TORCH:

    class _ResBlock(nn.Module):
        def __init__(self, dim, p=0.1):
            super().__init__()
            self.fc1 = nn.Linear(dim, dim)
            self.fc2 = nn.Linear(dim, dim)
            self.ln = nn.LayerNorm(dim)
            self.drop = nn.Dropout(p)
            self.act = nn.ReLU()

        def forward(self, x):
            h = self.act(self.fc1(x))
            h = self.drop(self.fc2(h))
            return self.act(self.ln(x + h))

    class ValueNet(nn.Module):
        """MLP with residual blocks over the flat state encoding.

        A residual MLP rather than a plain stack because the encoding is wide
        (~1900) and a few residual blocks train more stably than a deep plain
        net at this width; it is still tiny (~1M params) and evaluated in
        batches per decision, so inference is a single small GEMM on the GPU.
        """

        def __init__(self, in_dim, hidden=256, blocks=3, p=0.1):
            super().__init__()
            self.in_dim = in_dim
            self.hidden = hidden
            self.blocks_n = blocks
            self.stem = nn.Sequential(
                nn.Linear(in_dim, hidden), nn.LayerNorm(hidden), nn.ReLU())
            self.blocks = nn.ModuleList(
                [_ResBlock(hidden, p) for _ in range(blocks)])
            self.head = nn.Linear(hidden, 1)

        def forward(self, x):
            h = self.stem(x)
            for b in self.blocks:
                h = b(h)
            return self.head(h).squeeze(-1)

    def save_checkpoint(path, model, meta=None):
        obj = {
            "state_dict": model.state_dict(),
            "in_dim": model.in_dim,
            "hidden": model.hidden,
            "blocks": model.blocks_n,
            "margin_scale": MARGIN_NORM,
            "meta": meta or {},
        }
        torch.save(obj, path)

    def load_checkpoint(path, device="cpu"):
        obj = torch.load(path, map_location=device, weights_only=False)
        net = ValueNet(obj["in_dim"], obj["hidden"], obj["blocks"])
        net.load_state_dict(obj["state_dict"])
        net.to(device)
        net.eval()
        return net, obj


class NeuralValue:
    """Batched inference wrapper used by :class:`NeuralBot`.

    ``value(encodings)`` takes a list of flat float lists (each length
    ``in_dim``) and returns a python list of predicted margins in CULTURE
    units (i.e. already multiplied back by :data:`MARGIN_NORM`).
    """

    def __init__(self, model, device="cpu"):
        if not HAVE_TORCH:
            raise ImportError("torch is required for NeuralValue")
        self.model = model
        self.device = device
        self.model.eval()

    @classmethod
    def from_checkpoint(cls, path, device="cpu"):
        model, _ = load_checkpoint(path, device)
        return cls(model, device)

    def value(self, encodings):
        if not encodings:
            return []
        with torch.no_grad():
            x = self._to_tensor(encodings)
            y = self.model(x)
            return (y.float().cpu() * MARGIN_NORM).tolist()

    def _to_tensor(self, encodings):
        """List-of-lists -> tensor via numpy.

        Measured on the desktop, 200 x 1897 rows: ``torch.tensor(list)`` 28 ms,
        ``torch.from_numpy(np.asarray(...))`` 16 ms, and the forward pass
        itself is only 4-12 ms.  The conversion, not the net, is the cost of
        neural search, so it is worth the extra import.  Falls back to
        ``torch.tensor`` if numpy is unavailable or the rows are ragged.
        """
        try:
            import numpy as np
            a = np.asarray(encodings, dtype=np.float32)
            if a.ndim != 2:
                raise ValueError("ragged encodings")
            t = torch.from_numpy(a)
            return t.to(self.device) if self.device != "cpu" else t
        except Exception:
            return torch.tensor(encodings, dtype=torch.float32,
                                device=self.device)
