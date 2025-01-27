use alloy_primitives::{Address, Bytes};
use colorchoice::ColorChoice;
use revm::{
    context::{BlockEnv, CfgEnv, TxEnv},
    context_interface::{
        result::{ExecutionResult, HaltReason},
        TransactTo,
    },
    interpreter::interpreter::EthInterpreter,
    specification::hardfork::SpecId,
    Context, Database, DatabaseCommit, ExecuteCommitEvm, JournaledState,
};
use revm_inspector::{exec::InspectCommitEvm, Inspector};
use revm_inspectors::tracing::{TraceWriter, TraceWriterConfig, TracingInspector};

pub type ContextDb<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB, JournaledState<DB>, ()>;

pub fn write_traces(tracer: &TracingInspector) -> String {
    write_traces_with(tracer, TraceWriterConfig::new().color_choice(ColorChoice::Never))
}

pub fn write_traces_with(tracer: &TracingInspector, config: TraceWriterConfig) -> String {
    let mut w = TraceWriter::with_config(Vec::<u8>::new(), config);
    w.write_arena(tracer.traces()).expect("failed to write traces to Vec<u8>");
    String::from_utf8(w.into_writer()).expect("trace writer wrote invalid UTF-8")
}

pub fn print_traces(tracer: &TracingInspector) {
    // Use `println!` so that the output is captured by the test runner.
    println!("{}", write_traces_with(tracer, TraceWriterConfig::new()));
}

/// Deploys a contract with the given code and deployer address.
pub fn deploy_contract<DB: Database + DatabaseCommit>(
    context: &mut ContextDb<DB>,
    code: Bytes,
    deployer: Address,
    spec: SpecId,
) -> ExecutionResult<HaltReason> {
    context.modify_tx(|tx| {
        tx.caller = deployer;
        tx.gas_limit = 1000000;
        tx.kind = TransactTo::Create;
        tx.data = code;
    });
    context.modify_cfg(|cfg| cfg.spec = spec);

    let out = context.exec_commit_previous().expect("Expect to be executed");
    context.tx.nonce += 1;
    out
}

/// Deploys a contract with the given code and deployer address.
pub fn inspect_deploy_contract<DB: Database + DatabaseCommit, I>(
    context: &mut ContextDb<DB>,
    code: Bytes,
    deployer: Address,
    spec: SpecId,
    inspector: I,
) -> ExecutionResult<HaltReason>
where
    I: for<'a> Inspector<&'a mut ContextDb<DB>, EthInterpreter>,
{
    context.modify_cfg(|cfg| cfg.spec = spec);
    let output = context
        .inspect_commit(
            TxEnv {
                caller: deployer,
                gas_limit: 1000000,
                kind: TransactTo::Create,
                data: code,
                ..Default::default()
            },
            inspector,
        )
        .expect("Expect to be executed");
    context.tx.nonce += 1;
    output
}
