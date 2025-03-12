use alloy_primitives::{Address, Bytes};
use colorchoice::ColorChoice;
use revm::{
    context::{BlockEnv, CfgEnv, Evm, TxEnv},
    context_interface::{
        result::{ExecutionResult, HaltReason},
        TransactTo,
    },
    handler::{instructions::EthInstructions, EthPrecompiles, EvmTr},
    interpreter::interpreter::EthInterpreter,
    primitives::hardfork::SpecId,
    Context, Database, DatabaseCommit, ExecuteCommitEvm, InspectCommitEvm, Inspector, Journal,
};
use revm_inspectors::tracing::{TraceWriter, TraceWriterConfig, TracingInspector};

pub type ContextDb<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB, Journal<DB>, ()>;

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

pub type EvmDb<DB, INSP> =
    Evm<ContextDb<DB>, INSP, EthInstructions<EthInterpreter, ContextDb<DB>>, EthPrecompiles>;

/// Deploys a contract with the given code and deployer address.
pub fn deploy_contract<DB: Database + DatabaseCommit>(
    evm: &mut EvmDb<DB, ()>,
    code: Bytes,
    deployer: Address,
    spec: SpecId,
) -> ExecutionResult<HaltReason> {
    evm.ctx().modify_tx(|tx| {
        tx.caller = deployer;
        tx.gas_limit = 1000000;
        tx.kind = TransactTo::Create;
        tx.data = code;
    });
    evm.ctx().modify_cfg(|cfg| cfg.spec = spec);

    let out = evm.replay_commit().expect("Expect to be executed");
    evm.modify_tx(|tx| {
        tx.nonce += 1;
    });
    out
}

/// Deploys a contract with the given code and deployer address.
pub fn inspect_deploy_contract<DB: Database + DatabaseCommit, INSP: Inspector<ContextDb<DB>>>(
    evm: &mut EvmDb<DB, INSP>,
    code: Bytes,
    deployer: Address,
    spec: SpecId,
) -> ExecutionResult<HaltReason> {
    evm.ctx().modify_cfg(|cfg| cfg.spec = spec);
    evm.ctx().modify_tx(|tx| {
        *tx = TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Create,
            data: code,
            ..Default::default()
        };
    });
    let output = evm.inspect_commit_previous().expect("Expect to be executed");

    evm.ctx().modify_tx(|tx| {
        tx.nonce += 1;
    });
    output
}
