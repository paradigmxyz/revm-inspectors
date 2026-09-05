# Empty tracer RPC regression

`test-empty-tracer.py` deploys a contract and mines a call on an isolated dev
chain with a funded, unlocked account. It tests `debug_traceTransaction`,
`debug_traceBlockByNumber`, `debug_traceBlockByHash`, and `debug_traceCall`.
For each method it compares an absent tracer name with `tracer: ""`, using
default options and all 16 combinations of `enableMemory`, `disableStack`,
`disableStorage`, and `enableReturnData`. It checks field omission and populated
memory, stack, storage, and return data, plus named-tracer and whitespace dispatch.
The contract runtime is also exercised through `DebugInspector` by
`test_debug_empty_tracer` in the regular Rust integration suite.

Example with a disposable Geth dev chain (requires `geth` and Python 3.9+):

```sh
geth --dev --http --http.addr 127.0.0.1 --http.port 18547 --http.api eth,debug,web3 --ipcdisable
```

In another terminal:

```sh
python3 scripts/test-empty-tracer.py http://127.0.0.1:18547
```

The script sends two transactions; point it only at an isolated dev chain.
Use the same command with the RPC URL of a patched Reth dev node for the Reth
dependency-bump PR. A mismatch exits nonzero. No external nodes or binaries are
required by the normal Rust test suite.

## Geth comparison, 2026-09-05

Tested an unmodified Geth built from `github.com/ethereum/go-ethereum/cmd/geth@v1.17.5`:

```text
Geth/v1.17.5-stable/darwin-arm64/go1.25.1
FAIL debug_traceTransaction: 17/17 empty-name requests
FAIL debug_traceBlockByNumber: 17/17 empty-name requests
FAIL debug_traceBlockByHash: 17/17 empty-name requests
FAIL debug_traceCall: 17/17 empty-name requests
PASS named tracer and whitespace dispatch
```

Absent-name requests passed the opcode logger and option assertions. Empty-name
requests failed with a JavaScript syntax error (for block tracing, reported in
the transaction result): `SyntaxError: (anonymous): Line 1:3 Unexpected token )`.
This is an observed Geth/spec discrepancy, not evidence that Geth already
implements the requested behavior. Geth's
[`traceTx` dispatch](https://github.com/ethereum/go-ethereum/blob/v1.17.5/eth/tracers/api.go#L981)
selects the struct logger only for `config.Tracer == nil`.
The [execution-apis schema](https://github.com/ethereum/execution-apis/blob/main/src/schemas/opcode-tracer.yaml)
requires it for absent **or empty** tracer names.

Reth RPC execution has not yet been tested with this patch. The inspector fix
must first be included in the current release line and consumed by Reth; the
library regression runs locally with and without JS support.
