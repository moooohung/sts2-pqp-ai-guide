Burn 0.22.1 (latest stable, 2026-10-26). Local additive patch:
`ModuleOptimizer::load_record_on` restores optimizer state on an explicit device.
The upstream `load_record` API remains unchanged. Remove when upstream exposes it.
