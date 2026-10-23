//! JS frame values, bounded memory reads and execution refunds.
use super::*;

/// Runs a tracer over `tx` against `db`, returning the tracer's result.
fn trace_tx(code: &str, db: CacheDB<EmptyDB>, tx: TxEnv) -> serde_json::Value {
    let insp = JsInspector::new(code.to_string(), serde_json::Value::Null).unwrap();
    let mut evm = revm::Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
        .with_db(db)
        .build_mainnet_with_inspector(insp);
    let res = evm.inspect_tx(tx).expect("pass without error");
    let (ctx, inspector) = evm.ctx_inspector();
    let tx = ctx.tx().clone();
    let block = ctx.block().clone();
    inspector.json_result(res, &tx, &block, ctx.db_mut()).unwrap()
}

fn funded_db() -> CacheDB<EmptyDB> {
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        Address::ZERO,
        AccountInfo { balance: U256::from(1e18), ..Default::default() },
    );
    db
}

#[test]
fn test_selfdestruct_frame_describes_the_destruction() {
    let contract = Address::repeat_byte(0xaa);
    let beneficiary = Address::repeat_byte(0xcc);
    let mut db = funded_db();
    // PUSH20 <beneficiary>, SELFDESTRUCT
    let mut code = vec![0x73];
    code.extend_from_slice(beneficiary.as_slice());
    code.push(0xff);
    db.insert_account_info(
        contract,
        AccountInfo {
            balance: U256::from(5),
            code: Some(Bytecode::new_legacy(code.into())),
            ..Default::default()
        },
    );

    let tracer = r#"{f:[],step:function(){},fault:function(){},enter:function(fr){this.f.push([fr.getType(),toHex(fr.getFrom()),toHex(fr.getTo()),fr.getValue().toString(),fr.getGas(),toHex(fr.getInput())])},exit:function(){},result:function(){return this.f}}"#;
    let res = trace_tx(
        tracer,
        db,
        TxEnv { gas_limit: 1_000_000, kind: TransactTo::Call(contract), ..Default::default() },
    );
    assert_eq!(
        res,
        json!([[
            "SELFDESTRUCT",
            format!("{contract:#x}"),
            format!("{beneficiary:#x}"),
            "5",
            0,
            "0x"
        ]])
    );
}

#[test]
fn test_delegate_call_frame_reports_the_inherited_value() {
    let outer = Address::repeat_byte(0x01);
    let target = Address::repeat_byte(0x02);
    let mut db = funded_db();
    // PUSH1 0 x4, PUSH20 <target>, GAS, DELEGATECALL, STOP
    let mut code = hex!("6000600060006000").to_vec();
    code.push(0x73);
    code.extend_from_slice(target.as_slice());
    code.extend_from_slice(&hex!("5af400"));
    db.insert_account_info(
        outer,
        AccountInfo { code: Some(Bytecode::new_legacy(code.into())), ..Default::default() },
    );
    db.insert_account_info(
        target,
        AccountInfo { code: Some(Bytecode::new_legacy(hex!("00").into())), ..Default::default() },
    );

    let tracer = r#"{f:[],step:function(){},fault:function(){},enter:function(fr){this.f.push([fr.getType(),fr.getValue().toString()])},exit:function(){},result:function(){return this.f}}"#;
    let res = trace_tx(
        tracer,
        db,
        TxEnv {
            gas_limit: 1_000_000,
            value: U256::from(7),
            kind: TransactTo::Call(outer),
            ..Default::default()
        },
    );
    assert_eq!(res, json!([["DELEGATECALL", "7"]]));
}

#[test]
fn test_ctx_to_is_set_for_a_failed_creation() {
    // Init code that reverts immediately: PUSH1 0, PUSH1 0, REVERT.
    let tracer = r#"{step:function(){},fault:function(){},result:function(ctx){return {to:toHex(ctx.to),err:ctx.error}}}"#;
    let res = trace_tx(
        tracer,
        funded_db(),
        TxEnv {
            gas_limit: 1_000_000,
            kind: TransactTo::Create,
            data: hex!("60006000fd").into(),
            ..Default::default()
        },
    );
    assert_eq!(res["to"], json!(format!("{:#x}", Address::ZERO.create(0))));
    assert_eq!(res["err"], json!("execution reverted"));
}

#[test]
fn test_memory_slice_zero_pads_out_of_bounds() {
    // Read 32 bytes of still-empty memory on the first step. go-ethereum zero-pads rather
    // than erroring, so the slice has length 32.
    let code = r#"{r:null,step:function(log){if(this.r!==null)return;this.r='len='+log.memory.slice(0,32).length},fault:function(){},result:function(){return this.r}}"#;
    let res = run_trace(code, Some(hex!("600160015500").into()), true);
    assert_eq!(res, json!("len=32"));
}

#[test]
fn test_memory_slice_padding_has_a_limit() {
    // Padding beyond the 1 MiB limit is rejected.
    let code = r#"{r:null,step:function(log){if(this.r!==null)return;try{log.memory.slice(0, 1024*1024 + 1);this.r='ok'}catch(e){this.r='threw'}},fault:function(){},result:function(){return this.r}}"#;
    let res = run_trace(code, Some(hex!("600160015500").into()), true);
    assert_eq!(res, json!("threw"));
}

/// Runs `outer` with slot 0 of every listed account preset to 1, returning one
/// `depth:op:refund` entry per step.
fn refund_steps(accounts: &[(Address, Vec<u8>)], outer: Address) -> Vec<String> {
    let mut db = CacheDB::new(EmptyDB::default());
    db.insert_account_info(
        Address::ZERO,
        AccountInfo { balance: U256::from(1e18), ..Default::default() },
    );
    for (addr, code) in accounts {
        db.insert_account_info(
            *addr,
            AccountInfo {
                code: Some(Bytecode::new_legacy(code.clone().into())),
                ..Default::default()
            },
        );
        db.insert_account_storage(*addr, U256::ZERO, U256::from(1)).unwrap();
    }
    let code = r#"{r:[],step:function(log){this.r.push(log.getDepth()+':'+log.op.toString()+':'+log.getRefund())},fault:function(){},result:function(){return this.r}}"#;
    serde_json::from_value(trace_tx(
        code,
        db,
        TxEnv { gas_limit: 1_000_000, kind: TransactTo::Call(outer), ..Default::default() },
    ))
    .unwrap()
}

/// `SSTORE slot0 = 0`, then `op` (CALL or DELEGATECALL) to `target`, then `STOP`.
fn clear_then_call(op: u8, target: Address) -> Vec<u8> {
    let mut code = hex!("6000600055").to_vec();
    code.extend_from_slice(&hex!("6000600060006000"));
    if op == 0xf1 {
        code.extend_from_slice(&hex!("6000")); // value
    }
    code.push(0x73);
    code.extend_from_slice(target.as_slice());
    code.extend_from_slice(&hex!("61ffff"));
    code.push(op);
    code.push(0x00);
    code
}

#[test]
fn test_get_refund_is_transaction_wide() {
    // The outer frame clears its slot (+4800), then calls an inner frame that clears its own
    // (+4800). Inside the inner frame the counter is 9600, not the inner frame's 4800; the
    // SSTORE step already includes its own refund, as go-ethereum meters it before `OnOpcode`.
    let (outer, inner) = (Address::repeat_byte(0xaa), Address::repeat_byte(0xbb));
    let steps = refund_steps(
        &[(outer, clear_then_call(0xf1, inner)), (inner, hex!("600060005500").to_vec())],
        outer,
    );
    assert!(steps.contains(&"1:SSTORE:4800".to_string()), "{steps:?}");
    assert!(steps.contains(&"2:SSTORE:9600".to_string()), "{steps:?}");
    assert!(steps.contains(&"2:STOP:9600".to_string()), "{steps:?}");
    assert_eq!(steps.last().unwrap(), "1:STOP:9600", "{steps:?}");
}

#[test]
fn test_get_refund_does_not_underflow_on_a_negative_frame_counter() {
    // The outer frame clears slot 0 (+4800), then a delegate call writes it back, which takes
    // the refund away again (-4800 + 2800). The delegated frame's own counter is -2000; the
    // transaction-wide counter is 2800, not an underflowed u64.
    let (outer, restorer) = (Address::repeat_byte(0xaa), Address::repeat_byte(0xcc));
    let steps = refund_steps(
        &[(outer, clear_then_call(0xf4, restorer)), (restorer, hex!("600160005500").to_vec())],
        outer,
    );
    assert!(steps.contains(&"2:SSTORE:2800".to_string()), "{steps:?}");
    assert!(steps.contains(&"2:STOP:2800".to_string()), "{steps:?}");
    assert_eq!(steps.last().unwrap(), "1:STOP:2800", "{steps:?}");
}

#[test]
fn test_get_refund_drops_a_reverted_frame() {
    // The inner frame clears its slot and reverts; its refund is discarded with it.
    let (outer, inner) = (Address::repeat_byte(0xaa), Address::repeat_byte(0xbb));
    let steps = refund_steps(
        &[(outer, clear_then_call(0xf1, inner)), (inner, hex!("600060005560006000fd").to_vec())],
        outer,
    );
    assert!(steps.contains(&"2:SSTORE:9600".to_string()), "{steps:?}");
    assert_eq!(steps.last().unwrap(), "1:STOP:4800", "{steps:?}");
}

#[test]
fn memory_slice_padding_boundaries() {
    let tracer = r#"{r:null,step:function(log){if(log.op.toString()!=='STOP')return;
        var a=log.memory.slice(0,33);var capped=log.memory.slice(32,32+1024*1024);var error=null;
        try{log.memory.slice(32,33+1024*1024)}catch(e){error=e.name}
        this.r=[a[0],a[32],capped.length,log.memory.slice(2**40,2**40).length,error];
    },fault:function(){},result:function(){return this.r}}"#;
    assert_eq!(
        run_trace(tracer, Some(hex!("60ff60005300").into()), true),
        json!([255, 0, 1024 * 1024, 0, "TypeError"])
    );
}

#[test]
fn refund_includes_all_ancestor_frames() {
    let outer = Address::repeat_byte(0xaa);
    let inner = Address::repeat_byte(0xbb);
    let restorer = Address::repeat_byte(0xcc);
    let steps = refund_steps(
        &[
            (outer, clear_then_call(0xf1, inner)),
            (inner, clear_then_call(0xf4, restorer)),
            (restorer, hex!("600160005500").to_vec()),
        ],
        outer,
    );
    assert_eq!(
        steps.iter().filter(|s| s.contains(":SSTORE:")).cloned().collect::<Vec<_>>(),
        ["1:SSTORE:4800", "2:SSTORE:9600", "3:SSTORE:7600"]
    );
    assert_eq!(steps.last().unwrap(), "1:STOP:7600");
}
