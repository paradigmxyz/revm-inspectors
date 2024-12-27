use alloy_primitives::{Address, Bytes};
use colorchoice::ColorChoice;
use revm::{
    context::{BlockEnv, CfgEnv, TxEnv},
    context_interface::{
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction, ResultAndState},
        DatabaseGetter, TransactTo,
    },
    handler::EthHandler,
    interpreter::interpreter::EthInterpreter,
    specification::hardfork::SpecId,
    Context, Database, DatabaseCommit, Evm, EvmCommit, EvmExec, JournaledState,
};
use revm_inspector::{inspector_handler, GetInspector, InspectorContext, InspectorMainEvm};
use revm_inspectors::tracing::{TraceWriter, TraceWriterConfig, TracingInspector};

pub type ContextDb<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB, JournaledState<DB>, ()>;

/// Executes the [EnvWithHandlerCfg] against the given [Database] without committing state changes.
pub fn inspect<DB, I>(
    context: &mut ContextDb<DB>,
    inspector: I,
) -> Result<ResultAndState<HaltReason>, EVMError<DB::Error, InvalidTransaction>>
where
    DB: Database,
    I: for<'a> GetInspector<&'a mut ContextDb<DB>, EthInterpreter>,
{
    let ctx = InspectorContext::new(context, inspector);
    let mut evm = InspectorMainEvm::new(ctx, inspector_handler());

    evm.exec()
}

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
        tx.transact_to = TransactTo::Create;
        tx.data = code;
    });
    context.modify_cfg(|cfg| cfg.spec = spec);

    let out = Evm::new(&mut *context, EthHandler::default())
        .exec_commit()
        .expect("Expect to be executed");
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
    I: for<'a> GetInspector<&'a mut ContextDb<DB>, EthInterpreter>,
{
    context.modify_tx(|tx| {
        *tx = TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create,
            data: code,
            ..Default::default()
        };
    });
    context.modify_cfg(|cfg| cfg.spec = spec);
    let output = inspect(context, inspector).expect("Expect to be executed");
    context.db().commit(output.state);
    context.tx.nonce += 1;
    output.result
}
