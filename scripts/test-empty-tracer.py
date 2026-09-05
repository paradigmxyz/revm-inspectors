#!/usr/bin/env python3
"""RPC regression for absent versus empty debug tracer names.

Run against an isolated dev node with a funded, unlocked account:
    python3 scripts/test-empty-tracer.py http://127.0.0.1:8545

Deploys a contract and sends one transaction. Requires eth and debug RPC APIs.
Uses only the Python standard library; exits nonzero on any mismatch.
The same test can run against Geth and Reth (with either JS feature setting).
"""

import argparse
import json
import time
import urllib.request

# Store 42 in memory, STATICCALL the identity precompile to populate return data,
# store 42 in slot zero, then return the memory word.
RUNTIME = "602a6000526020600060206000600461fffffa50602a60005560206000f3"


class RpcError(RuntimeError):
    """JSON-RPC error returned by the node."""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url", help="RPC URL of an isolated dev node")
    args = parser.parse_args()

    def rpc(method, params):
        request = urllib.request.Request(
            args.url,
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
            {"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            body = json.load(response)
        if "error" in body:
            raise RpcError(f"{method} {params}: {body['error']}")
        return body["result"]

    def send(transaction):
        tx_hash = rpc("eth_sendTransaction", [transaction])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            try:
                receipt = rpc("eth_getTransactionReceipt", [tx_hash])
            except RpcError as error:
                if "transaction indexing is in progress" not in str(error):
                    raise
                receipt = None
            if receipt is not None:
                assert receipt["status"] == "0x1", receipt
                return receipt
            time.sleep(0.1)
        raise AssertionError(f"transaction not mined within 30 seconds: {tx_hash}")

    print(rpc("web3_clientVersion", []), flush=True)
    accounts = rpc("eth_accounts", [])
    assert accounts, "requires a funded, unlocked dev account"
    sender = accounts[0]
    size = len(bytes.fromhex(RUNTIME))
    init = f"60{size:02x}600c60003960{size:02x}6000f3{RUNTIME}"
    deployment = send({"from": sender, "data": "0x" + init, "gas": "0x100000"})
    call = {"from": sender, "to": deployment["contractAddress"], "gas": "0x100000"}
    receipt = send(call)
    tx_hash = receipt["transactionHash"]

    configs = [{}] + [
        dict(zip(
            ("enableMemory", "disableStack", "disableStorage", "enableReturnData"),
            (bool(flags & (1 << bit)) for bit in range(4)),
        ))
        for flags in range(16)
    ]
    methods = [
        ("debug_traceTransaction", [tx_hash]),
        ("debug_traceBlockByNumber", [receipt["blockNumber"]]),
        ("debug_traceBlockByHash", [receipt["blockHash"]]),
        ("debug_traceCall", [call, "latest"]),
    ]
    failures = []
    for method, params in methods:
        method_failures = 0
        for config in configs:
            absent = rpc(method, params + [config])
            try:
                empty = rpc(method, params + [{**config, "tracer": ""}])
                assert absent == empty, (method, config, absent, empty)
            except (RpcError, AssertionError) as error:
                method_failures += 1
                failures.append(str(error))
            # Validate the default path even when the empty-name request failed.
            empty = absent
            if isinstance(empty, list):
                entries = [entry for entry in empty if entry["txHash"] == tx_hash]
                assert len(entries) == 1, empty
                empty = entries[0]["result"]
            assert empty["failed"] is False, empty
            assert int(empty["returnValue"].removeprefix("0x"), 16) == 42, empty
            logs = empty["structLogs"]
            assert logs and any(log["op"] == "SSTORE" for log in logs), logs
            # Check observable option behavior as well as equality of the two paths.
            for field, enabled in (
                ("memory", config.get("enableMemory", False)),
                ("stack", not config.get("disableStack", False)),
                ("storage", not config.get("disableStorage", False)),
                ("returnData", config.get("enableReturnData", False)),
            ):
                populated = any(log.get(field) not in (None, [], {}, "", "0x") for log in logs)
                assert populated == enabled, (method, config, field, logs)
                if not enabled:
                    assert all(field not in log for log in logs), (method, config, field)
        if method_failures:
            print(f"FAIL {method}: {method_failures}/{len(configs)} empty-name requests", flush=True)
        else:
            print(f"PASS {method}: absent == empty, defaults + 16 option combinations", flush=True)

    named = rpc("debug_traceTransaction", [tx_hash, {"tracer": "callTracer"}])
    assert named["type"] == "CALL" and "structLogs" not in named, named
    # Whitespace is a nonempty custom tracer expression and must still be rejected.
    for whitespace in (" ", "\t\n"):
        try:
            rpc("debug_traceTransaction", [tx_hash, {"tracer": whitespace}])
        except RpcError:
            pass
        else:
            raise AssertionError("whitespace unexpectedly selected a tracer")
    print("PASS named tracer and whitespace dispatch", flush=True)
    if failures:
        raise AssertionError(f"{len(failures)} mismatches; first failure: {failures[0]}")


if __name__ == "__main__":
    main()
