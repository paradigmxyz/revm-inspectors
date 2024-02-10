use revm::{
    inspector_handle_register,
    primitives::{EVMError, EnvWithHandlerCfg, ResultAndState},
    Database, GetInspector,
};

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
