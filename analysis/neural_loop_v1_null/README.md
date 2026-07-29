# Evidence for docs/NEURAL_LOOP_NULL.md

The 41-hour Stage-2 self-play run, 2026-07-27 14:26 to 2026-07-29 07:00, on the
RTX 3090 desktop, at repo commit `6e5061e`. 74 iterations, 20,700 gate games,
**zero promotions**, pooled candidate win rate **0.4413**.

| file | what |
|---|---|
| `curve.tsv` | one row per iteration: promoted, candidate win rate, CI, both culture means, reference win rates |
| `master.log` | the orchestrator's own log, every iteration's verdict |
| `train_it73.log` | the representative training log: train loss down monotonically while the reported held-out metric goes the wrong way |
| `gate_it73.log` | the representative gate: candidate loses 0.415 +/- 0.056, p = 0.0028, n=300 |
| `vacuity_probe.py` | the script that closed the case -- scores the incumbent on its own training pairs with **no training at all** |

`vacuity_probe.py` is the reusable part. Run it against any ranking-pair shard
set and its checkpoint:

    python vacuity_probe.py checkpoints/best.pt

It reports pair accuracy *before* the first gradient step, separately per data
source. On the failed run that number was **0.9764** on the self-play shards --
the target was a fixed point of the model being trained. Anything above ~0.95
means the objective has nothing to teach and the run is a no-op. The same check
now runs automatically as the `VACUITY` line in
`experiments/neural_train_rank.py`.

Full logs (1,111 files, 4.4 MB, including all 888 generation-worker logs and the
frozen `best.pt`) are archived outside the repo:

* Mac: `~/tta-ai-archive/neural_loop_v1_null_2026-07-29.tgz` and
  `~/tta-ai-archive/neural_loop_v1_best.pt`
* desktop: `C:\Users\micro\tta-ai\archive\run_v1_selfimitation_null\`
