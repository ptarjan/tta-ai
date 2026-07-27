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
:data:`MARGIN_SCALE`.  Margin, not win/loss, because it is a dense label that
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

#: final margins are ~[-250, 250]; /100 puts the regression target near unit
#: scale without clipping.
MARGIN_SCALE = 100.0


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
            "margin_scale": MARGIN_SCALE,
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
    units (i.e. already multiplied back by :data:`MARGIN_SCALE`).
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
            x = torch.tensor(encodings, dtype=torch.float32, device=self.device)
            y = self.model(x)
            return (y.float().cpu() * MARGIN_SCALE).tolist()
