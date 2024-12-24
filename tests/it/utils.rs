use alloy_primitives::{Address, Bytes, U256};
use colorchoice::ColorChoice;
use revm::{
    context::{BlockEnv, CfgEnv, TxEnv},
    context_interface::{
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction, ResultAndState},
        DatabaseGetter, TransactTo,
    },
    database_interface::EmptyDB,
    handler::EthHandler,
    interpreter::interpreter::EthInterpreter,
    specification::hardfork::SpecId,
    Context, Database, DatabaseCommit, Evm, EvmCommit, EvmExec, JournaledState, MainEvm,
};
use revm_database::CacheDB;
use revm_inspector::{
    inspector_handler, GetInspector, Inspector, InspectorContext, InspectorHandler,
    InspectorMainEvm,
};
use revm_inspectors::tracing::{
    TraceWriter, TraceWriterConfig, TracingInspector, TracingInspectorConfig,
};
use std::convert::Infallible;

// type TestDb = CacheDB<EmptyDB>;

// #[derive(Clone, Debug)]
// pub struct TestEvm {
//     pub context: Context<BlockEnv, TxEnv, CfgEnv, CacheDB<EmptyDB>>,
// }

// impl Default for TestEvm {
//     fn default() -> Self {
//         Self::new()
//     }
// }

// impl TestEvm {
//     pub fn new() -> Self {
//         let context = Context::default()
//             .modify_block_chained(|b| {
//                 b.gas_limit = U256::MAX;
//             })
//             .modify_tx_chained(|tx| {
//                 tx.gas_limit = u64::MAX;
//                 tx.gas_price = U256::ZERO;
//             })
//             .with_db(CacheDB::new(EmptyDB::default()))
//             .modify_cfg_chained(|cfg| {
//                 cfg.spec = SpecId::CANCUN;
//             });

//         Self { context }
//     }

//     pub fn new_with_spec_id(spec_id: SpecId) -> Self {
//         let mut evm = Self::new();
//         evm.context.modify_cfg(|cfg| cfg.spec = spec_id);
//         evm
//     }

//     pub fn env_with_tx(&self, tx_env: TxEnv) -> EnvWithHandlerCfg {
//         let mut env = self.env.clone();
//         env.tx = tx_env;
//         env
//     }

//     pub fn simple_deploy(&mut self, data: Bytes) -> Address {
//         self.deploy(data, TracingInspector::new(TracingInspectorConfig::default_geth()))
//             .expect("failed to deploy contract")
//     }

//     pub fn deploy<I: for<'a> GetInspector<&'a mut TestDb>>(
//         &mut self,
//         data: Bytes,
//         inspector: I,
//     ) -> Result<Address, EVMError<Infallible>> {
//         let (_, address) = self.try_deploy(data, inspector)?;
//         Ok(address.expect("failed to deploy contract"))
//     }

//     pub fn try_deploy<I: for<'a> GetInspector<&'a mut TestDb>>(
//         &mut self,
//         data: Bytes,
//         inspector: I,
//     ) -> Result<(ExecutionResult, Option<Address>), EVMError<Infallible>> {
//         self.env.tx.data = data;
//         self.env.tx.transact_to = TransactTo::Create;

//         let (ResultAndState { result, state }, env) = self.inspect(inspector)?;
//         self.db.commit(state);
//         self.env = env;
//         match &result {
//             ExecutionResult::Success { output, .. } => {
//                 let address = output.address().copied();
//                 Ok((result, address))
//             }
//             _ => Ok((result, None)),
//         }
//     }

//     pub fn call<I: for<'a> GetInspector<&'a mut TestDb>>(
//         &mut self,
//         address: Address,
//         data: Bytes,
//         inspector: I,
//     ) -> Result<ExecutionResult, EVMError<Infallible>> {
//         self.env.tx.data = data;
//         self.env.tx.transact_to = TransactTo::Call(address);
//         let (ResultAndState { result, state }, env) = self.inspect(inspector)?;
//         self.db.commit(state);
//         self.env = env;
//         Ok(result)
//     }

//     pub fn inspect<I: for<'a> GetInspector<&'a mut TestDb>>(
//         &mut self,
//         inspector: I,
//     ) -> Result<(ResultAndState, EnvWithHandlerCfg), EVMError<Infallible>> {
//         inspect(&mut self.db, self.env.clone(), inspector)
//     }
// }

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
        *tx = TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            transact_to: TransactTo::Create,
            data: code,
            ..Default::default()
        };
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
