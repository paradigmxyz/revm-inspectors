//! Hook errors and execution failures must remain distinguishable.
use super::*;

#[test]
fn test_hook_error_is_reported_instead_of_result() {
    // Every `step` throws; geth reports the error rather than the tracer's `result`.
    let code = r#"{step:function(){throw new Error('boom')},fault:function(){},result:function(){return 'unreachable'}}"#;

    let addr = Address::repeat_byte(0x01);
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        Address::ZERO,
        AccountInfo { balance: U256::from(1e18), ..Default::default() },
    );
    db.insert_account_info(
        addr,
        AccountInfo {
            code: Some(Bytecode::new_legacy(hex!("6001600100").into())),
            ..Default::default()
        },
    );

    let insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
    let mut evm = revm::Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
        .with_db(db)
        .build_mainnet_with_inspector(insp);
    let res = evm
        .inspect_tx(TxEnv {
            gas_price: 1024,
            gas_limit: 1_000_000,
            kind: TransactTo::Call(addr),
            ..Default::default()
        })
        .expect("pass without error");
    let (ctx, inspector) = evm.ctx_inspector();
    let tx = ctx.tx().clone();
    let block = ctx.block().clone();
    let err = inspector
        .json_result(res, &tx, &block, ctx.db_mut())
        .expect_err("a thrown hook must surface as an error, not a result");
    let msg = err.to_string();
    assert!(msg.starts_with("Error: boom    in server-side tracer function 'step'"), "{msg}");
}

#[test]
fn test_hook_error_keeps_the_error_kind() {
    // The thrown error's own kind leads the message, on one line without Boa's backtrace.
    let cases = [
        ("null.x", "TypeError: cannot convert 'null' or 'undefined' to object"),
        ("throw 'plain'", "Error: plain"),
        (
            "while (true) {}",
            "RangeError: reached the maximum number of iteration loops on this execution",
        ),
    ];
    for (body, expected) in cases {
        let code = format!(
            "{{step: function() {{ {body}; }}, fault: function() {{}}, result: function() {{ return null }}}}"
        );
        let err = run_trace_err(&code, None, true);
        let expected = format!("{expected}    in server-side tracer function 'step'");
        assert!(err.starts_with(&expected), "{err}");
        assert!(!err.contains('\n'), "{err}");
    }
}

#[test]
fn test_fault_fires_for_non_revert_execution_errors() {
    // A tracer that counts fault callbacks and records the last error. The step function is
    // present but does nothing, matching the common geth pattern.
    let code = r#"{n:0,e:null,step:function(){},fault:function(log){this.n++;this.e=log.getError()},result:function(){return {faults:this.n,err:this.e}}}"#;

    // PUSH1 0xff, JUMP -> invalid jump destination (an execution error, not a revert).
    let res = run_trace(code, Some(hex!("60ff56").into()), false);
    assert_eq!(res["faults"], json!(1), "invalid jump must fire fault: {res}");
    assert_eq!(res["err"], json!("invalid jump destination"), "{res}");

    // 0x0c is not a defined opcode.
    let res = run_trace(code, Some(hex!("0c").into()), false);
    assert_eq!(res["faults"], json!(1), "undefined opcode must fire fault: {res}");

    // REVERT still faults (regression guard).
    let res = run_trace(code, Some(hex!("60006000fd").into()), false);
    assert_eq!(res["faults"], json!(1), "revert must still fire fault: {res}");
    assert_eq!(res["err"], json!("execution reverted"), "{res}");

    // A normal STOP does not fault.
    let res = run_trace(code, Some(hex!("00").into()), true);
    assert_eq!(res["faults"], json!(0), "a clean stop must not fault: {res}");
}

#[test]
fn test_fault_fires_without_a_step_function() {
    // A tracer that defines `fault` and `result` but no `step`. go-ethereum still delivers
    // fault callbacks to it on execution errors.
    let code = r#"{n:0,e:null,fault:function(log){this.n++;this.e=log.getError()},result:function(){return {faults:this.n,err:this.e}}}"#;

    // PUSH1 0xff, JUMP -> invalid jump destination.
    let res = run_trace(code, Some(hex!("60ff56").into()), false);
    assert_eq!(res["faults"], json!(1), "invalid jump must fault without a step fn: {res}");
    assert_eq!(res["err"], json!("invalid jump destination"), "{res}");

    // A clean STOP does not fault.
    let res = run_trace(code, Some(hex!("00").into()), true);
    assert_eq!(res["faults"], json!(0), "a clean stop must not fault: {res}");
}

/// `ctx.error` for a halting execution is the phrase the root frame reports elsewhere, not the
/// halt reason's Rust type name such as `OutOfGas(Basic)` or `StackUnderflow`.
#[test]
fn test_ctx_error_is_a_stable_phrase() {
    let code = r#"{step:function(){},fault:function(){},result:function(ctx){return ctx.error}}"#;

    // PUSH1 0xff, JUMP: an invalid jump destination.
    assert_eq!(
        run_trace(code, Some(hex!("60ff56").into()), false),
        json!("invalid jump destination")
    );
    // ADD with an empty stack.
    assert_eq!(run_trace(code, Some(hex!("01").into()), false), json!("stack underflow"));
    // The designated invalid instruction.
    assert_eq!(run_trace(code, Some(hex!("fe").into()), false), json!("invalid opcode: INVALID"));
}

/// `fuse` clears the recorded root result, so a later transaction that halts without running a
/// frame (e.g. an OP deposit rejected before execution) does not report the previous
/// transaction's error.
#[test]
fn test_ctx_error_does_not_leak_across_fuse() {
    use revm::context_interface::result::{HaltReason, ResultGas};

    let code = r#"{step:function(){},fault:function(){},result:function(ctx){return ctx.error}}"#;
    let addr = Address::repeat_byte(0x01);
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        addr,
        AccountInfo {
            // PUSH1 0xff, JUMP: an invalid jump destination.
            code: Some(Bytecode::new_legacy(hex!("60ff56").into())),
            ..Default::default()
        },
    );
    let insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
    let mut evm = revm::Context::mainnet().with_db(db).build_mainnet_with_inspector(insp);
    let res = evm
        .inspect_tx(TxEnv {
            gas_limit: 1_000_000,
            kind: TransactTo::Call(addr),
            ..Default::default()
        })
        .unwrap();
    let (ctx, inspector) = evm.ctx_inspector();
    let tx = ctx.tx().clone();
    let block = ctx.block().clone();
    assert_eq!(
        inspector.json_result(res, &tx, &block, ctx.db_mut()).unwrap(),
        json!("invalid jump destination")
    );

    inspector.fuse().unwrap();
    // A halt for which no frame ran, so no root result was recorded.
    let halted = ResultAndState {
        result: ExecutionResult::Halt {
            reason: HaltReason::InvalidFEOpcode,
            gas: ResultGas::default(),
            logs: Vec::new(),
        },
        state: Default::default(),
    };
    assert_eq!(
        inspector.json_result(halted, &tx, &block, ctx.db_mut()).unwrap(),
        json!("execution halted: InvalidFEOpcode")
    );
}

#[test]
fn fault_follows_step_without_dropping_the_failed_instruction() {
    let script = r#"{events:[],step:function(l){this.events.push(['step',l.getPC(),l.getError()||null])},fault:function(l){this.events.push(['fault',l.getPC(),l.getError()])},result:function(){return this.events}}"#;
    assert_eq!(
        run_trace(script, Some(hex!("60ff56").into()), false),
        json!([["step", 0, null], ["step", 2, null], ["fault", 2, "invalid jump destination"]])
    );
    // Validation failures go to step with an error, without a subsequent fault.
    assert_eq!(
        run_trace(script, Some(hex!("01").into()), false),
        json!([["step", 0, "stack underflow"]])
    );
}

#[test]
fn fault_only_keeps_the_instruction_metadata() {
    let script = r#"{events:[],fault:function(l){this.events.push([l.getPC(),l.op.toString(),l.getCost()])},result:function(){return this.events}}"#;
    assert_eq!(run_trace(script, Some(hex!("60ff5600").into()), false), json!([[2, "JUMP", 8]]));
}

#[test]
fn runtime_oog_without_root_hooks_keeps_the_reason() {
    let caller = Address::repeat_byte(0xbb);
    let recipient = Address::repeat_byte(0xcc);
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(caller, AccountInfo::from_balance(U256::from(1_000_000)));
    let inspector = JsInspector::new(
        "{fault:function(){},result:function(ctx){return ctx.error}}".into(),
        json!(null),
    )
    .unwrap();
    let mut evm = revm::Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::AMSTERDAM))
        .with_db(db)
        .build_mainnet_with_inspector(inspector);
    let tx = TxEnv {
        caller,
        kind: TransactTo::Call(recipient),
        value: U256::ONE,
        gas_limit: 200_000,
        ..Default::default()
    };
    let result = evm.inspect_tx(tx.clone()).unwrap();
    assert!(result.result.is_halt());
    let (ctx, inspector) = evm.ctx_inspector();
    assert!(inspector.root_instruction_result.is_none());
    let block = ctx.block().clone();
    assert_eq!(
        inspector.json_result(result, &tx, &block, ctx.db_mut()).unwrap(),
        json!("out of gas")
    );
}

#[test]
fn throwing_frame_hooks_preserve_execution_and_reset_with_fuse() {
    let target = Address::repeat_byte(0xaa);
    let child = Address::repeat_byte(0xbb);
    for hook in ["enter", "exit"] {
        let script = format!(
            "{{enter:function(){{{enter}}},exit:function(){{{exit}}},fault:function(){{}},result:function(){{return 42}}}}",
            enter = if hook == "enter" { "throw new Error('boom')" } else { "" },
            exit = if hook == "exit" { "throw new Error('boom')" } else { "" }
        );
        let mut code = hex!("6000600060006000600073").to_vec();
        code.extend_from_slice(child.as_slice());
        code.extend_from_slice(&hex!("61fffff15000"));
        let mut db = CacheDB::new(EmptyDB::default());
        db.insert_account_info(
            target,
            AccountInfo::default().with_code(Bytecode::new_raw(code.into())),
        );
        db.insert_account_info(
            child,
            AccountInfo::default().with_code(Bytecode::new_raw(hex!("600160005500").into())),
        );
        let inspector = JsInspector::new(script, json!(null)).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(SpecId::CANCUN))
            .with_db(db)
            .build_mainnet_with_inspector(inspector);
        let tx =
            TxEnv { kind: TransactTo::Call(target), gas_limit: 1_000_000, ..Default::default() };
        let result = evm.inspect_tx(tx.clone()).unwrap();
        assert!(result.result.is_success());
        assert_eq!(result.state[&child].storage[&U256::ZERO].present_value, U256::ONE);
        let (ctx, inspector) = evm.ctx_inspector();
        let block = ctx.block().clone();
        let first = inspector
            .json_result(result.clone(), &tx, &block, ctx.db_mut())
            .unwrap_err()
            .to_string();
        let second = inspector
            .json_result(result.clone(), &tx, &block, ctx.db_mut())
            .unwrap_err()
            .to_string();
        assert_eq!(first, second);
        assert!(
            first.starts_with(&format!("Error: boom    in server-side tracer function '{hook}'")),
            "{first}"
        );
        inspector.fuse().unwrap();
        assert_eq!(inspector.json_result(result, &tx, &block, ctx.db_mut()).unwrap(), json!(42));
    }
}

#[test]
fn the_first_hook_error_stops_later_callbacks() {
    let script = r#"{step:function(){throw new Error('step failed')},fault:function(){throw new Error('fault failed')},result:function(){return 'unreachable'}}"#;
    let error = run_trace_err(script, Some(hex!("fe").into()), false);
    assert!(
        error.starts_with("Error: step failed    in server-side tracer function 'step'"),
        "{error}"
    );
}

#[test]
fn throwing_fault_is_reported() {
    let script = r#"{fault:function(){throw new TypeError('fault failed')},result:function(){return 'unreachable'}}"#;
    let error = run_trace_err(script, Some(hex!("60ff56").into()), false);
    assert!(
        error.starts_with("TypeError: fault failed    in server-side tracer function 'fault'"),
        "{error}"
    );
}
