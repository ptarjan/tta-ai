"""Flatten experiments/behaviour_{2,3,4}p.json into a side-by-side table.

Owned by the heuristics-doc agent. Read-only over experiments/.
Usage: python3 analysis/heuristics_compare.py [substring-filter]
"""
import json, sys, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def flat(o, p=""):
    out = {}
    if isinstance(o, dict):
        for k, v in o.items():
            out.update(flat(v, p + "/" + str(k)))
    elif isinstance(o, list):
        out[p] = o
    else:
        out[p] = o
    return out


def main():
    filt = sys.argv[1] if len(sys.argv) > 1 else ""
    data = {}
    for k in (2, 3, 4):
        with open(os.path.join(ROOT, "experiments", f"behaviour_{k}p.json")) as f:
            data[k] = flat(json.load(f))
    keys = sorted(set().union(*(d.keys() for d in data.values())))
    print(f"{'key':<58}{'2p':>12}{'3p':>12}{'4p':>12}")
    for key in keys:
        if filt and filt not in key:
            continue
        row = []
        for k in (2, 3, 4):
            v = data[k].get(key)
            row.append("-" if v is None else (f"{v:>12.3f}" if isinstance(v, float) else f"{str(v):>12}"))
        print(f"{key:<58}" + "".join(row))


if __name__ == "__main__":
    main()
