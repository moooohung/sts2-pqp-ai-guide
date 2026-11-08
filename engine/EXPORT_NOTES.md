# Engine export notes

Source: private workspace `sts2_native`, commit `1ffac8bf1ce96665cf0ebce3017767b0b8049d29`.
Goal: build `sts2_advisor_bridge` (`cargo build --release -p sts2_advisor_bridge`) and run the tests
that need no private data. Everything below was removed or changed for that.

## Crates

Kept: `advisor_bridge`, `aiplay`, `human_replay`, `native_obs`, `policy`, `sim`, `vecenv`, `wire`,
and `vendor/burn-optim-0.22.1` (patched upstream Burn crate, see its `STS2_PATCH.md`).

Not included: `bench`, `pyenv`, `sim_core`, `sim_int1`, `stack`, `trainer`, the root Python extension
crate, and every Python, shell and tooling script. `vendor/burn-cubecl*` is dropped together with the
CUDA backend.

## Removed from kept crates

- `aiplay`: `publish`, `ingest`, `shadow`, `appo`, `native_appo`, `native_ingest`, `contract`, `policy`
  modules and all binaries (they talk to the training server, upload pipeline and checkpoint registry),
  the PyTorch/zip/tempfile/flate2/time features, `FORMAT.md`, JavaScript parity scripts, recorded fixtures.
- `policy`: the `gpu` (CUDA/CubeCL) and `pytorch` features, `load_r7`, the CUDA kernel and CUDA tests,
  and the two checkpoint-check binaries.
- `vecenv`: the `int1` backend (`sim_int1` is a generated artifact of the old simulator), the
  `proof_parity`/`prefix_divergence` modules and the recorder examples.
- `human_replay`: the CLI `main.rs` and the tests that read recorded game runs.
- `aiplay` and `advisor_bridge`: the decision-search tests that replay a recorded game run to a combat
  choice (`live::fixture`, the `test-fixture` feature and the bridge's C ABI search tests).
- `sim`: all examples, and the integration tests that need recorded captures or parity fixtures.
- `native_obs`: the tests and examples that need recorded captures.

## Game-derived data

- `crates/sim/data/v2/{card,event,potion,relic}.json` are reduced to class, entry and the constructor
  calls of each model's `CanonicalVars`. The originals hold the decompiled member text of every model.
  The generated code is byte-identical to a build from the originals.
- `crates/sim/src/run/selector_shapes.keys` held the game's option text. It is replaced by
  `selector_shapes.sha256`, the SHA-256 of each key, checked through `recorded_selector_key`.
- `crates/sim/data/csharp_meta.json`: the local source path in `source` was replaced by a description.
- The remaining `crates/sim/data/*.json`, `native_obs/data/original_ids.json` and the ported rules in
  `crates/sim/src` are the rule tables and ports the simulator needs to compile.

## Tests

Tests that failed at the source commit itself (in the private workspace) were removed: seven
`sts2_sim` integration tests and two `vecenv` lane tests. One `PlayerRewards` initializer in three
integration tests was completed to match the struct. See the export commit message.
