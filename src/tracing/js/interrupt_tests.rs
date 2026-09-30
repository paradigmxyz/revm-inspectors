//! Cooperative cancellation at EVM and JavaScript callback boundaries.

use super::*;
use alloy_primitives::hex;
use boa_engine::NativeFunction;
use boa_gc::{Finalize, Trace};
use core::convert::Infallible;
use revm::{
    context::TxEnv,
    context_interface::result::EVMError,
    database::{CacheDB, EmptyDB},
    state::{AccountInfo, Bytecode},
    InspectEvm, MainBuilder, MainContext,
};

const TRACER: &str = "{ fault: function() {}, result: function() { return 42; } }";
const TARGET: Address = Address::with_last_byte(0x42);

#[test]
fn unset_interrupt_preserves_results() {
    for interrupt in [None, Some(JsInspectorInterrupt::default())] {
        let mut inspector = JsInspector::new(TRACER.into(), serde_json::Value::Null).unwrap();
        if let Some(interrupt) = interrupt {
            inspector = inspector.with_interrupt(interrupt);
        }
        let (mut inspector, result) = run(inspector, &hex!("60015000"), TransactTo::Call(TARGET));
        let result = result.unwrap();
        assert!(result.result.is_success());
        assert_eq!(
            inspector
                .json_result(
                    result,
                    &TxEnv::default(),
                    &revm::context::BlockEnv::default(),
                    &EmptyDB::default()
                )
                .unwrap(),
            serde_json::json!(42)
        );
    }
}

#[test]
fn pre_cancelled_calls_and_creates_abort() {
    for kind in [
        TransactTo::Call(TARGET),
        // Empty account and identity precompile have no interpreter steps.
        TransactTo::Call(Address::with_last_byte(0x43)),
        TransactTo::Call(Address::with_last_byte(4)),
        TransactTo::Create,
    ] {
        let interrupt = JsInspectorInterrupt::new();
        interrupt.interrupt();
        let inspector = JsInspector::new(TRACER.into(), serde_json::Value::Null)
            .unwrap()
            .with_interrupt(interrupt);
        let (_, result) = run(inspector, &hex!("60015000"), kind);
        assert_interrupted(result);
    }
}

#[test]
fn interrupt_stops_steps_without_js_step_callback() {
    let interrupt = JsInspectorInterrupt::new();
    let mut inspector = JsInspector::new(TRACER.into(), serde_json::Value::Null)
        .unwrap()
        .with_interrupt(interrupt.clone());
    let mut context = revm::Context::mainnet();
    let mut interp = Interpreter::default();
    // Signal from another thread after the inspector has been created.
    std::thread::spawn(move || interrupt.interrupt()).join().unwrap();
    inspector.step(&mut interp, &mut context);
    assert_eq!(
        interp.bytecode.action().as_ref().unwrap().instruction_result(),
        Some(InstructionResult::FatalExternalError)
    );
    assert_eq!(context.error, Err(ContextError::Custom("JavaScript tracing interrupted".into())));
}

#[test]
fn interrupt_during_step_replaces_pending_actions() {
    for (code, op) in [
        (&hex!("60015000")[..], "PUSH1"),
        (&hex!("00")[..], "STOP"),
        (&hex!("5f5ff3")[..], "RETURN"),
        (&hex!("5f5ffd")[..], "REVERT"),
        (&hex!("5f5f5f5f5f600461fffff100")[..], "CALL"),
        (&hex!("5f5f5ff000")[..], "CREATE"),
    ] {
        let script = format!(
            "{{ step: function(log) {{ if (log.op.toString() === '{op}') interrupt(); }},
                fault: function() {{ interrupt(); }},
                enter: function() {{ throw 'must not enter'; }},
                result: function() {{ return 42; }} }}"
        );
        let inspector = interruptible(&script);
        let (_, result) = run(inspector, code, TransactTo::Call(TARGET));
        assert_interrupted(result);
    }
}

#[test]
fn interrupt_during_enter_and_exit_aborts() {
    for callback in ["enter", "exit"] {
        for code in [
            // Identity precompile and empty initcode exercise frames without EVM steps.
            &hex!("5f5f5f5f5f600461fffff100")[..],
            &hex!("5f5f5ff000")[..],
            // SELFDESTRUCT invokes enter/exit outside of step_end.
            &hex!("6043ff")[..],
        ] {
            let script = format!(
                "{{ {callback}: function() {{ interrupt(); }},
                    fault: function() {{}}, result: function() {{ return 42; }} }}"
            );
            let (_, result) = run(interruptible(&script), code, TransactTo::Call(TARGET));
            assert_interrupted(result);
        }
    }
}

#[test]
fn interrupt_before_step_end_skips_callback_and_preserves_error() {
    let interrupt = JsInspectorInterrupt::new();
    let mut inspector = JsInspector::new(
        "{step: function() { throw 'must not run'; }, fault: function() {}, result: function() {}}"
            .into(),
        serde_json::Value::Null,
    )
    .unwrap()
    .with_interrupt(interrupt.clone());
    let mut context = revm::Context::mainnet();
    let mut interp = Interpreter::default();
    inspector.step(&mut interp, &mut context);
    assert!(inspector.step_pending);
    interp.bytecode.set_action(InterpreterAction::new_halt(InstructionResult::Stop, interp.gas));
    context.error = Err(ContextError::Custom("original error".into()));
    interrupt.interrupt();
    inspector.step_end(&mut interp, &mut context);
    assert!(!inspector.step_pending);
    assert_eq!(context.error, Err(ContextError::Custom("original error".into())));
    assert_eq!(
        interp.bytecode.action().as_ref().unwrap().instruction_result(),
        Some(InstructionResult::FatalExternalError)
    );
}

#[test]
fn interrupt_survives_clone_and_fuse() {
    let interrupt = JsInspectorInterrupt::new();
    let mut inspector = JsInspector::new(TRACER.into(), serde_json::Value::Null)
        .unwrap()
        .with_interrupt(interrupt.clone());
    let cloned = inspector.try_clone().unwrap();
    inspector.fuse().unwrap();
    interrupt.interrupt();
    assert!(matches!(inspector.fuse(), Err(JsInspectorError::Interrupted)));
    assert!(matches!(inspector.try_clone(), Err(JsInspectorError::Interrupted)));
    for inspector in [inspector, cloned] {
        let (_, result) = run(inspector, &hex!("00"), TransactTo::Call(TARGET));
        assert_interrupted(result);
    }
}

#[test]
fn interrupt_rejects_result_collection() {
    for script in [
        TRACER,
        "{fault: function() {}, result: function() { interrupt(); return 42; }}",
        "{fault: function() {}, result: function() { return {toJSON: function() { interrupt(); return 42; }}; }}",
    ] {
        let (mut inspector, result) = run(interruptible(script), &hex!("00"), TransactTo::Call(TARGET));
        let result = result.unwrap();
        if script == TRACER {
            inspector.interrupt.as_ref().unwrap().interrupt();
        }
        assert!(matches!(
            inspector.json_result(result, &TxEnv::default(), &revm::context::BlockEnv::default(), &EmptyDB::default()),
            Err(JsInspectorError::Interrupted)
        ));
    }
}

/// An atomic flag contains no garbage-collected JavaScript values.
#[derive(Trace, Finalize)]
struct InterruptSignal(#[unsafe_ignore_trace] JsInspectorInterrupt);

fn interruptible(script: &str) -> JsInspector {
    let interrupt = JsInspectorInterrupt::new();
    let mut inspector = JsInspector::new(script.into(), serde_json::Value::Null)
        .unwrap()
        .with_interrupt(interrupt.clone());
    inspector
        .ctx
        .register_global_builtin_callable(
            js_string!("interrupt"),
            0,
            NativeFunction::from_copy_closure_with_captures(
                |_, _, signal, _| {
                    signal.0.interrupt();
                    Ok(JsValue::undefined())
                },
                InterruptSignal(interrupt),
            ),
        )
        .unwrap();
    inspector
}

fn run(
    inspector: JsInspector,
    code: &[u8],
    kind: TransactTo,
) -> (JsInspector, Result<ResultAndState, EVMError<Infallible>>) {
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(
        TARGET,
        AccountInfo::default().with_code(Bytecode::new_raw(code.to_vec().into())),
    );
    let mut evm = revm::Context::mainnet().with_db(db).build_mainnet_with_inspector(inspector);
    let result = evm.inspect_tx(TxEnv { gas_limit: 1_000_000, kind, ..Default::default() });
    (evm.into_inspector(), result)
}

fn assert_interrupted(result: Result<ResultAndState, EVMError<Infallible>>) {
    assert!(
        matches!(result, Err(EVMError::Custom(message)) if message == "JavaScript tracing interrupted")
    );
}
