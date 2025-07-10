use revm::{
    context::TxEnv,
    database::CacheDB,
    database_interface::EmptyDB,
    primitives::hardfork::SpecId,
    state::{AccountInfo, Bytecode},
    InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::js::JsInspector;
use alloy_primitives::{hex, Address, U256};
use revm::context_interface::TransactTo;

fn setup_test_evm() -> (CacheDB<EmptyDB>, TxEnv) {
    let mut db = CacheDB::new(EmptyDB::new());
    let addr = Address::repeat_byte(0x01);
    
    // Insert the caller
    db.insert_account_info(
        Address::ZERO,
        AccountInfo { balance: U256::from(1e18), ..Default::default() },
    );
    
    // Insert a contract with some bytecode that does basic operations
    let bytecode = hex!("608060405260043610603f5760003560e01c80632e64cec11460445780636057361d14605e5780636f760f41146069578063b4a24f50146070575b600080fd5b604c60005481565b60405190815260200160405180910390f35b6067606336600460a6565b6000555b005b6067600080fd5b606760001981565b634e487b7160e01b600052603260045260246000fd5b634e487b7160e01b600052604160045260246000fd5b6000602082840312156091578081fd5b81356001600160a01b038116811460a5578182fd5b939250505056").into();
    
    db.insert_account_info(
        addr,
        AccountInfo {
            code: Some(Bytecode::new_legacy(bytecode)),
            ..Default::default()
        },
    );
    
    let tx = TxEnv {
        gas_price: 1024,
        gas_limit: 1_000_000,
        gas_priority_fee: None,
        kind: TransactTo::Call(addr),
        ..Default::default()
    };
    
    (db, tx)
}

fn main() {
    let (db, tx) = setup_test_evm();
    
    // Choose which benchmark to profile by uncommenting one:
    
    // Profile simple step counter
    profile_simple_tracer(db, tx);
    
    // profile_db_access_tracer(db, tx);
    // profile_heavy_operations_tracer(db, tx);
    // profile_memory_operations_tracer(db, tx);
    // profile_stack_operations_tracer(db, tx);
    // profile_enter_exit_tracer(db, tx);
}

fn profile_simple_tracer(db: CacheDB<EmptyDB>, tx: TxEnv) {
    let simple_tracer = r#"{
        count: 0,
        step: function() { this.count += 1; },
        fault: function() {},
        result: function() { return this.count; }
    }"#;
    
    // Run many iterations for profiling
    for _ in 0..1000 {
        let inspector = JsInspector::new(simple_tracer.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db.clone())
            .build_mainnet_with_inspector(inspector);
        
        let _res = evm.inspect_tx(tx.clone()).expect("execution failed");
    }
}

fn profile_db_access_tracer(db: CacheDB<EmptyDB>, tx: TxEnv) {
    let db_tracer = r#"{
        balances: [],
        step: function(log, db) {
            if (log.op.toString() === 'CALL') {
                var addr = log.contract.getAddress();
                this.balances.push(db.getBalance(addr));
            }
        },
        fault: function() {},
        result: function() { return this.balances.length; }
    }"#;
    
    for _ in 0..1000 {
        let inspector = JsInspector::new(db_tracer.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db.clone())
            .build_mainnet_with_inspector(inspector);
        
        let _res = evm.inspect_tx(tx.clone()).expect("execution failed");
    }
}

fn profile_heavy_operations_tracer(db: CacheDB<EmptyDB>, tx: TxEnv) {
    let heavy_tracer = r#"{
        steps: [],
        step: function(log, db) {
            this.steps.push({
                pc: log.getPC(),
                op: log.op.toString(),
                gas: log.getGas(),
                cost: log.getCost(),
                depth: log.getDepth(),
                error: log.getError(),
                stack: log.stack.length(),
                memory: log.memory.length()
            });
        },
        fault: function() {},
        result: function() { return this.steps.length; }
    }"#;
    
    for _ in 0..1000 {
        let inspector = JsInspector::new(heavy_tracer.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db.clone())
            .build_mainnet_with_inspector(inspector);
        
        let _res = evm.inspect_tx(tx.clone()).expect("execution failed");
    }
}

fn profile_memory_operations_tracer(db: CacheDB<EmptyDB>, tx: TxEnv) {
    let memory_tracer = r#"{
        memAccesses: 0,
        step: function(log, db) {
            if (log.memory.length() > 0) {
                var slice = log.memory.slice(0, Math.min(32, log.memory.length()));
                this.memAccesses++;
            }
        },
        fault: function() {},
        result: function() { return this.memAccesses; }
    }"#;
    
    for _ in 0..1000 {
        let inspector = JsInspector::new(memory_tracer.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db.clone())
            .build_mainnet_with_inspector(inspector);
        
        let _res = evm.inspect_tx(tx.clone()).expect("execution failed");
    }
}

fn profile_stack_operations_tracer(db: CacheDB<EmptyDB>, tx: TxEnv) {
    let stack_tracer = r#"{
        stackOps: 0,
        step: function(log, db) {
            var stackLen = log.stack.length();
            if (stackLen > 0) {
                var top = log.stack.peek(0);
                this.stackOps++;
            }
        },
        fault: function() {},
        result: function() { return this.stackOps; }
    }"#;
    
    for _ in 0..1000 {
        let inspector = JsInspector::new(stack_tracer.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db.clone())
            .build_mainnet_with_inspector(inspector);
        
        let _res = evm.inspect_tx(tx.clone()).expect("execution failed");
    }
}

fn profile_enter_exit_tracer(db: CacheDB<EmptyDB>, tx: TxEnv) {
    let enter_exit_tracer = r#"{
        enters: 0,
        exits: 0,
        step: function() {},
        fault: function() {},
        enter: function(frame) {
            this.enters++;
            var from = frame.getFrom();
            var to = frame.getTo();
            var gas = frame.getGas();
        },
        exit: function(frame) {
            this.exits++;
            var gasUsed = frame.getGasUsed();
            var error = frame.getError();
        },
        result: function() { return {enters: this.enters, exits: this.exits}; }
    }"#;
    
    for _ in 0..1000 {
        let inspector = JsInspector::new(enter_exit_tracer.to_string(), serde_json::Value::Null).unwrap();
        let mut evm = revm::Context::mainnet()
            .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
            .with_db(db.clone())
            .build_mainnet_with_inspector(inspector);
        
        let _res = evm.inspect_tx(tx.clone()).expect("execution failed");
    }
}