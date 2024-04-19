use alloy_primitives::{Address, Bytes, U256};
use colorchoice::ColorChoice;
use revm::{
    db::{CacheDB, EmptyDB},
    inspector_handle_register,
    primitives::{
        BlockEnv, EVMError, Env, EnvWithHandlerCfg, ExecutionResult, HandlerCfg, Output,
        ResultAndState, SpecId, TransactTo, TxEnv,
    },
    Database, DatabaseCommit, GetInspector,
};
use revm_inspectors::tracing::TracingInspector;
use std::convert::Infallible;

type TestDb = CacheDB<EmptyDB>;

#[derive(Clone, Debug)]
pub struct TestEvm {
    pub db: TestDb,
    pub env: EnvWithHandlerCfg,
}

impl Default for TestEvm {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEvm {
    pub fn new() -> Self {
        let db = CacheDB::new(EmptyDB::default());
        let env = EnvWithHandlerCfg::new(
            Box::new(Env {
                block: BlockEnv { gas_limit: U256::MAX, ..Default::default() },
                tx: TxEnv { gas_limit: u64::MAX, gas_price: U256::ZERO, ..Default::default() },
                ..Default::default()
            }),
            HandlerCfg::new(SpecId::CANCUN),
        );
        Self { db, env }
    }

    pub fn deploy<I: for<'a> GetInspector<&'a mut TestDb>>(
        &mut self,
        data: Bytes,
        inspector: I,
    ) -> Result<Address, EVMError<Infallible>> {
        self.env.tx.data = data;
        self.env.tx.transact_to = TransactTo::Create;

        let (ResultAndState { result, state }, env) = self.inspect(inspector)?;
        self.db.commit(state);
        let address = match result {
            ExecutionResult::Success { output, .. } => match output {
                Output::Create(_, address) => address.unwrap(),
                _ => panic!("Create failed"),
            },
            _ => panic!("Execution failed"),
        };
        self.env = env;
        Ok(address)
    }

    pub fn call<I: for<'a> GetInspector<&'a mut TestDb>>(
        &mut self,
        address: Address,
        data: Bytes,
        inspector: I,
    ) -> Result<ExecutionResult, EVMError<Infallible>> {
        self.env.tx.data = data;
        self.env.tx.transact_to = TransactTo::Call(address);
        let (ResultAndState { result, state }, env) = self.inspect(inspector)?;
        self.db.commit(state);
        self.env = env;
        Ok(result)
    }

    pub fn inspect<I: for<'a> GetInspector<&'a mut TestDb>>(
        &mut self,
        inspector: I,
    ) -> Result<(ResultAndState, EnvWithHandlerCfg), EVMError<Infallible>> {
        inspect(&mut self.db, self.env.clone(), inspector)
    }
}

/// Executes the [EnvWithHandlerCfg] against the given [Database] without committing state changes.
pub fn inspect<DB, I>(
    db: DB,
    env: EnvWithHandlerCfg,
    inspector: I,
) -> Result<(ResultAndState, EnvWithHandlerCfg), EVMError<DB::Error>>
where
    DB: Database,
    I: GetInspector<DB>,
{
    let mut evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(inspector)
        .with_env_with_handler_cfg(env)
        .append_handler_register(inspector_handle_register)
        .build();
    let res = evm.transact()?;
    let (_, env) = evm.into_db_and_env_with_handler_cfg();
    Ok((res, env))
}

pub fn write_traces(tracer: &TracingInspector) -> String {
    write_traces_with(tracer, ColorChoice::Never)
}

pub fn write_traces_with(tracer: &TracingInspector, color: ColorChoice) -> String {
    let mut w = revm_inspectors::tracing::TraceWriter::new(Vec::<u8>::new()).use_colors(color);
    w.write_arena(tracer.get_traces()).expect("failed to write traces to Vec<u8>");
    String::from_utf8(w.into_writer()).expect("trace writer wrote invalid UTF-8")
}

pub fn print_traces(tracer: &TracingInspector) {
    // Use `println!` so that the output is captured by the test runner.
    println!("{}", write_traces_with(tracer, ColorChoice::Auto));
}
