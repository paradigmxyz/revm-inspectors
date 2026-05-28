# JS Tracer Stable-Object Investigation

## TL;DR

- The old `js-tracer` hot path rebuilt fresh Boa wrapper objects on every `step`, `fault`, `enter`, and `exit` callback.
- That per-callback object construction was the main source of overhead, especially for opcode-heavy tracers.
- The fix was to create stable reusable JS objects once and update shared Rust-side backing state before each callback.
- The external JS tracer API did not change; scripts still use the same Geth-style methods and object shapes.
- A new benchmark suite was added covering synthetic accessor-heavy workloads, Rundler-shaped workloads, real Rundler tracer scripts, and a real mainnet AA transaction replay.
- The optimization materially improves all measured cases:
- `step_noop`: `157.18 ms` -> `4.1913 ms` (`37.5x`)
- `step_accessors`: `230.39 ms` -> `38.457 ms` (`6.0x`)
- `rundler_v06_real`: `41.989 ms` -> `15.463 ms` (`2.72x`)
- `mainnet_aa_v06_real`: `9.7643 ms` -> `3.6274 ms` (`2.69x`)
- The mainnet AA improvement is smaller than the synthetic upper-bound cases because that benchmark includes more non-tracer work, not because the benchmark is broken.

## Summary

This writeup documents the investigation into `revm-inspectors` JavaScript tracer overhead, the issues found in the existing implementation, the fixes made to address them, and the benchmark coverage added to measure the impact.

The core result is that the JS tracer hot path was spending a large amount of time rebuilding short-lived Boa object graphs for every `step`, `fault`, `enter`, and `exit` callback. Replacing those per-callback object allocations with stable, reusable JS wrapper objects materially improves performance across synthetic microbenchmarks, Rundler-shaped workloads, real Rundler tracer scripts, and a replay of a real Ethereum mainnet Account Abstraction transaction.

## Investigation

The investigation started from an observed mismatch between the cost of `js-tracer` execution and the amount of actual EVM work being done. The slowdown was especially visible in tracers that receive a callback on every opcode and then repeatedly access `log.op`, `log.stack`, `log.memory`, `log.contract`, and database methods such as `db.exists`, `db.getCode`, and `db.getState`.

Reading the upstream implementation showed that every callback rebuilt a fresh JS-facing wrapper graph:

- `StepLog` rebuilt `op`, `memory`, `stack`, and `contract` wrapper objects for every opcode callback.
- `CallFrame` rebuilt JS wrappers for every nested `enter` callback.
- `FrameResult` rebuilt JS wrappers for every `exit` callback.
- `EvmDbRef` rebuilt a fresh JS database object for every callback that exposed `db`.
- Each wrapper object also rebuilt its function properties and captured values again.

This meant the tracer was paying a large Boa allocation and closure-construction cost even when the JavaScript script itself was simple. In other words, a lot of the measured time was Rust-side wrapper churn rather than tracer logic.

That cost is especially painful because `step` callbacks happen once per opcode, so even small amounts of per-callback allocation quickly dominate the total runtime.

## Issues Found

The main issues found were:

- The `step` callback path rebuilt the JS wrapper graph on every opcode, even though the JavaScript-visible shape is stable across callbacks.
- Nested wrapper objects such as `log.op`, `log.memory`, `log.stack`, and `log.contract` were also recreated every time instead of reusing object identity and updating only the backing values.
- The database wrapper passed to JS was rebuilt for each callback instead of using a stable wrapper that points at the current `EvmDbRef`.
- The same pattern existed for `enter` and `exit` callback objects, so call-heavy tracers paid similar avoidable overhead.
- The existing benchmark coverage was too synthetic to justify upstreaming the change on its own. It did not include real Rundler tracer scripts or a real-world AA transaction replay.

## Fixes Made

### 1. Added Stable, Reusable JS Wrapper Objects

The main change lives in [src/tracing/js/bindings.rs](file:///home/boney/Developer/revm-inspectors/src/tracing/js/bindings.rs) and [src/tracing/js/mod.rs](file:///home/boney/Developer/revm-inspectors/src/tracing/js/mod.rs).

Instead of rebuilding JS wrapper objects for every callback, the implementation now creates long-lived wrapper objects once and updates shared Rust-side state before each callback.

The new reusable wrappers are:

- `ReusableStepLog`
- `ReusableCallFrame`
- `ReusableFrameResult`
- `ReusableEvmDb`

Each reusable wrapper owns a stable `JsObject` plus shared mutable state. The function properties on those objects read from the current state at call time. That means the tracer script still sees the same Geth-style API, but we avoid re-allocating the JS object graph for every callback.

### 2. Preserved Existing Access Semantics

The optimization does not flatten or remove the existing callback API. Scripts still call the same methods such as:

- `log.op.toNumber()`
- `log.op.toString()`
- `log.stack.peek(0)`
- `log.memory.slice(...)`
- `log.memory.getUint(...)`
- `log.contract.getAddress()`
- `frame.getInput()`
- `db.getState(...)`

The change is only in how those methods are backed internally. Instead of capturing freshly built values every callback, they now read from the updated shared state.

### 3. Kept Ephemeral Safety Checks Intact

The JS tracer bindings already use guarded references so that JS closures cannot safely outlive the Rust values they expose. That behavior was preserved.

Stack, memory, and DB access still go through the existing guard-based machinery. The optimization reduces allocation churn, but it does not widen the lifetime of the underlying EVM data.

### 4. Wired the Reusable Objects Into the Inspector Hot Path

`JsInspector` now creates reusable wrappers during initialization and updates them in the callback entry points:

- `try_step`
- `try_fault`
- `try_enter`
- `try_exit`

That keeps the callback work focused on updating state and invoking JavaScript rather than constructing a new wrapper graph each time.

### 5. Upstream Compatibility Fixes

Porting the stable-object approach into upstream `revm-inspectors` also required a small compatibility adjustment in [src/tracing/debug.rs](file:///home/boney/Developer/revm-inspectors/src/tracing/debug.rs#L302-L305), where the `DebugInspector` implementation now constrains `CTX` with `Db: DatabaseRef` so the JS tracer path compiles cleanly in upstream.

### 6. Warning Cleanup

After the hot-path refactor, several legacy `into_js_object` helpers were only referenced by unit tests. Those helpers and their small helper macros were scoped to `#[cfg(test)]`, and one unused parameter was renamed to `_ctx`, so the benchmark build is now warning-free.

## Benchmarks Added

Benchmark support was added in [Cargo.toml](file:///home/boney/Developer/revm-inspectors/Cargo.toml#L53-L61) and [benches/js_tracer.rs](file:///home/boney/Developer/revm-inspectors/benches/js_tracer.rs).

The benchmark suite now covers several levels of realism.

### Synthetic Step-Heavy Baselines

- `step_noop`
- `step_accessors`

These isolate the callback machinery itself.

`step_noop` measures the cost of invoking a `step` callback that does almost nothing.

`step_accessors` measures the cost of a callback that exercises common JS tracer accessors such as opcode decoding, stack length, memory length, and DB existence checks.

### Rundler-Shaped Synthetic Workloads

- `rundler_v06_style`
- `rundler_v07_style`

These benches model the callback and accessor patterns seen in Rundler-style validation tracers, including memory access, stack peeks, storage reads, code reads, nested calls, and log-driven phase transitions.

They are more realistic than the original synthetic microbenchmarks while still being fully self-contained and deterministic.

### Real Rundler JavaScript Tracers

- `rundler_v06_real`
- `rundler_v07_real`

These benches execute the real transpiled Rundler tracer scripts, loaded via:

- `RUNDLER_V06_TRACER_PATH`
- `RUNDLER_V07_TRACER_PATH`

That closes the gap between synthetic benchmarks and production-style tracer logic.

### Real Mainnet AA Replay

- `mainnet_aa_v06_real`

This bench replays a real Ethereum mainnet Account Abstraction transaction using a prestate fixture in [testdata/repro/tx-aa-handleops-mainnet.json](file:///home/boney/Developer/revm-inspectors/testdata/repro/tx-aa-handleops-mainnet.json).

The replay uses the real Rundler v0.6 tracer script and reconstructs the execution DB from prestate data in the same general style as the upstream prestate repro tests.

The chosen transaction is:

- Transaction hash: `0x1e664de3785a6fe2fc71c4a790fadb3b935ba9f6306b1a6a908d703457a84c12`
- EntryPoint: `0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789`
- Block number: `24,921,426`

This benchmark matters because it is the closest thing in the suite to a real end-to-end AA tracing workload.

## Benchmark Methodology

The benchmark comparison was run in two configurations:

- Before: upstream `HEAD` tracer implementation without the stable-object optimization
- After: the patched implementation with reusable stable JS wrappers

Both configurations ran the same benchmark harness and the same tracer scripts.

For the Rundler script benches, the real tracer JavaScript files came from a local Rundler checkout transpiled to JavaScript.

For the mainnet AA bench, the same prestate fixture and transaction environment were used in both runs.

## Results

The table below uses the Criterion point estimate from each run.

| Benchmark | Before | After | Improvement | Speedup |
| --- | ---: | ---: | ---: | ---: |
| `step_noop` | 157.18 ms | 4.1913 ms | 97.3% | 37.50x |
| `step_accessors` | 230.39 ms | 38.457 ms | 83.3% | 5.99x |
| `rundler_v06_style` | 1.3781 ms | 0.59711 ms | 56.7% | 2.31x |
| `rundler_v07_style` | 1.3495 ms | 0.65381 ms | 51.6% | 2.06x |
| `rundler_v06_real` | 41.989 ms | 15.463 ms | 63.2% | 2.72x |
| `rundler_v07_real` | 2.0798 ms | 1.1739 ms | 43.6% | 1.77x |
| `mainnet_aa_v06_real` | 9.7643 ms | 3.6274 ms | 62.9% | 2.69x |

## Why These Benchmarks Show Real Improvement

The improvements line up with the expected cost model of the change.

### Worst-Case Synthetic Benches Improve the Most

`step_noop` and `step_accessors` spend most of their time in callback setup overhead rather than tracer logic. Because the optimization directly targets wrapper allocation and callback object construction, these benches show the largest gains.

That is useful because it confirms the root cause: the old implementation was dominated by wrapper churn, not by EVM execution or by the user tracer script itself.

### Rundler-Shaped Benches Improve, But Less Dramatically

The Rundler-style synthetic benches still improve by roughly 2x. That is smaller than the pure microbenchmarks because they include more real tracer work, including actual storage reads, code reads, memory slicing, and nested-call behavior.

Even so, a roughly 2x reduction is significant because it shows the optimization still matters after the benchmark includes realistic tracer logic.

### Real Rundler Scripts Confirm the Improvement Is Not Synthetic-Only

The real Rundler v0.6 and v0.7 tracers are important validation because they use actual production tracer code rather than a hand-written approximation.

The fact that both improve, and that v0.6 improves by more than 2.7x, strongly suggests the optimization is addressing a real production bottleneck.

The smaller gain for v0.7 also makes sense. Different scripts stress different parts of the interface, so a tracer with fewer expensive callback interactions will naturally benefit less than one that heavily exercises step-time wrappers.

### The Real Mainnet AA Replay Is the Strongest Evidence

The `mainnet_aa_v06_real` benchmark is the most persuasive result in the suite because it uses:

- a real Ethereum mainnet transaction,
- real prestate-derived account and storage contents,
- a real Rundler tracer script,
- and the same local replay machinery in both before and after runs.

That benchmark still improves by about 2.7x, which shows the optimization survives contact with a realistic AA execution path and is not just an artifact of synthetic test design.

## Why the Change Works

The old implementation paid for all of the following on every callback:

- allocating wrapper objects,
- allocating function objects,
- wiring methods onto wrapper objects,
- capturing fresh values into closures,
- rebuilding nested wrapper graphs.

The new implementation pays those construction costs once and then only updates the backing state before invoking JavaScript.

That changes the hot path from:

1. build JS wrapper graph,
2. capture EVM state into fresh closures,
3. call JavaScript,

to:

1. update shared Rust-side state,
2. call JavaScript through stable objects.

Because opcode tracing can invoke `step` thousands of times in a single execution, removing that repeated wrapper construction yields large cumulative savings.

## Reproducing the Benchmarks

Build the benchmark target:

```bash
cargo bench --bench js_tracer --features js-tracer --no-run
```

Run the full suite with real Rundler tracers:

```bash
RUNDLER_V06_TRACER_PATH=/path/to/validationTracerV0_6.js \
RUNDLER_V07_TRACER_PATH=/path/to/validationTracerV0_7.js \
cargo bench --bench js_tracer --features js-tracer -- --noplot
```

Run only the real mainnet AA replay bench:

```bash
RUNDLER_V06_TRACER_PATH=/path/to/validationTracerV0_6.js \
RUNDLER_V07_TRACER_PATH=/path/to/validationTracerV0_7.js \
cargo bench --bench js_tracer --features js-tracer mainnet_aa_v06_real -- --noplot
```

## Conclusion

The investigation showed that the dominant issue was not the JavaScript tracer logic itself, but the repeated construction of Boa wrapper objects on every callback.

Reusing stable JS objects while updating shared backing state fixes that hot-path inefficiency without changing the external tracer API.

The added benchmark suite demonstrates the improvement at four levels:

- microbenchmark callback overhead,
- Rundler-shaped synthetic behavior,
- real Rundler tracer scripts,
- and a replay of a real Ethereum mainnet AA transaction.

Those results make a strong case that the optimization is worth upstreaming into `revm-inspectors` rather than maintaining as a long-lived fork-only patch.
