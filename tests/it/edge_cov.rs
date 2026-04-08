//! Edge coverage tests

use alloy_primitives::{hex, Address, U256};
use revm::{
    context::TxEnv,
    context_interface::{
        result::{ExecutionResult, Output},
        ContextTr, TransactTo,
    },
    database::CacheDB,
    database_interface::EmptyDB,
    handler::EvmTr,
    inspector::InspectorEvmTr,
    primitives::hardfork::SpecId,
    Context, DatabaseCommit, InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::{
    edge_cov::{CmpOperands, EdgeCovInspector},
    tracing::{TracingInspector, TracingInspectorConfig},
};

#[test]
fn test_edge_coverage() {
    /*
    contract X {
        function Y(bool yes) external {
            for (uint256 i = 0; i < 255; i++) {
                if (yes) {
                    break;
                }
            }
        }
    }
    */

    let code = hex!("6080604052348015600f57600080fd5b5060b580601d6000396000f3fe6080604052348015600f57600080fd5b506004361060285760003560e01c8063f42e8cdd14602d575b600080fd5b603c60383660046058565b603e565b005b60005b60ff811015605457816054576001016041565b5050565b600060208284031215606957600080fd5b81358015158114607857600080fd5b939250505056fea2646970667358221220a206d90c473b6930258d5789495c41b79941b5334c47a76b6e618d3571716d5164736f6c634300081c0033");
    let deployer = Address::ZERO;

    let ctx = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::LONDON)
        .with_db(CacheDB::new(EmptyDB::default()));

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());

    let mut evm = ctx.build_mainnet_with_inspector(&mut insp);

    // Create contract
    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Create,
            data: code.into(),
            ..Default::default()
        })
        .unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    evm.ctx().db_mut().commit(res.state);

    let acc = evm.ctx().db_mut().load_account(deployer).unwrap();
    acc.info.balance = U256::from(u64::MAX);

    let tx = TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        kind: TransactTo::Call(addr),
        nonce: 1,
        // 'cast cd "Y(bool)" true'
        data: hex!("f42e8cdd0000000000000000000000000000000000000000000000000000000000000001")
            .into(),
        ..Default::default()
    };

    let insp = EdgeCovInspector::new();
    let mut evm = evm.with_inspector(insp);
    let res = evm.inspect_tx(tx).unwrap();
    assert!(res.result.is_success());

    let counts = evm.inspector().get_hitcount();
    assert_eq!(counts.iter().filter(|&x| *x != 0).count(), 11);
    assert_eq!(counts.iter().filter(|&x| *x == 1).count(), 11);

    evm.inspector().reset();
    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 100000000,
            kind: TransactTo::Call(addr),
            nonce: 1,
            // 'cast cd "Y(bool)" false'
            data: hex!("f42e8cdd0000000000000000000000000000000000000000000000000000000000000000")
                .into(),
            ..Default::default()
        })
        .unwrap();
    assert!(res.result.is_success());

    // There should be 13 non-zero counts and two edges that have been hit 255 times.
    let mut counts = evm.inspector.into_hitcount();

    counts.sort();
    assert_eq!(counts[counts.len() - 1], 255);
    assert_eq!(counts[counts.len() - 2], 255);
    assert_eq!(counts.iter().filter(|&x| *x != 0).count(), 13);
}

#[test]
fn test_cmp_log_basic() {
    /*
    contract CmpTest {
        function compare(uint256 a, uint256 b) external pure returns (bool) {
            return a == b;
        }
    }
    */
    // Compiled bytecode from Solidity 0.8.33
    let code = hex!("6080604052348015600e575f5ffd5b5061012e8061001c5f395ff3fe6080604052348015600e575f5ffd5b50600436106026575f3560e01c8063f360234c14602a575b5f5ffd5b60406004803603810190603c91906092565b6054565b604051604b919060e1565b60405180910390f35b5f818314905092915050565b5f5ffd5b5f819050919050565b6074816064565b8114607d575f5ffd5b50565b5f81359050608c81606d565b92915050565b5f5f6040838503121560a55760a46060565b5b5f60b0858286016080565b925050602060bf858286016080565b9150509250929050565b5f8115159050919050565b60db8160c9565b82525050565b5f60208201905060f25f83018460d4565b9291505056fea26469706673582212201aa69cc881fa3f0ed6fd2a2d01f2a9efd71de5f7aebe97a5904f3177eb12c99764736f6c63430008210033");
    let deployer = Address::ZERO;

    let ctx = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::CANCUN)
        .with_db(CacheDB::new(EmptyDB::default()));

    let mut insp = TracingInspector::new(TracingInspectorConfig::default_geth());
    let mut evm = ctx.build_mainnet_with_inspector(&mut insp);

    // Create contract
    let res = evm
        .inspect_tx(TxEnv {
            caller: deployer,
            gas_limit: 1000000,
            kind: TransactTo::Create,
            data: code.into(),
            ..Default::default()
        })
        .unwrap();
    let addr = match res.result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Create(_, addr) => addr.unwrap(),
            _ => panic!("Create failed"),
        },
        _ => panic!("Execution failed"),
    };
    evm.ctx().db_mut().commit(res.state);

    let acc = evm.ctx().db_mut().load_account(deployer).unwrap();
    acc.info.balance = U256::from(u64::MAX);

    // Call compare(42, 100)
    let tx = TxEnv {
        caller: deployer,
        gas_limit: 100000000,
        kind: TransactTo::Call(addr),
        nonce: 1,
        // 'cast calldata "compare(uint256,uint256)" 42 100'
        data: hex!("f360234c000000000000000000000000000000000000000000000000000000000000002a0000000000000000000000000000000000000000000000000000000000000064")
            .into(),
        ..Default::default()
    };

    let insp = EdgeCovInspector::new();
    let mut evm = evm.with_inspector(insp);
    let res = evm.inspect_tx(tx).unwrap();
    assert!(res.result.is_success());

    // Check that CmpLog captured some comparisons
    let cmp_log = evm.inspector().get_cmp_log();
    assert!(!cmp_log.is_empty(), "CmpLog should capture comparison operands");

    // Should have captured the comparison values 42 and 100
    let has_42 = cmp_log.iter().any(|cmp| cmp.op1 == U256::from(42) || cmp.op2 == U256::from(42));
    let has_100 = cmp_log.iter().any(|cmp| cmp.op1 == U256::from(100) || cmp.op2 == U256::from(100));
    assert!(has_42 || has_100, "CmpLog should capture operand values from comparison");
}

#[test]
fn test_cmp_log_into_parts() {
    let inspector = EdgeCovInspector::new();

    // Verify initial state
    assert!(inspector.get_cmp_log().is_empty());
    assert!(inspector.get_hitcount().iter().all(|&x| x == 0));

    // Test into_parts
    let (hitcount, cmp_log) = inspector.into_parts();
    assert_eq!(hitcount.len(), 65536); // MAX_EDGE_COUNT
    assert!(cmp_log.is_empty());
}

#[test]
fn test_cmp_log_reset() {
    /*
    contract LtTest {
        function lessThan(uint256 a, uint256 b) external pure returns (bool) {
            return a < b;
        }
    }
    */
    let code = hex!("6080604052348015600f57600080fd5b5060043610601c5760003560e01c8063021c06c0146021575b600080fd5b6031603336600460505650565b60405190151581526020015b60405180910390f35b60006020828403121560615750565b5091905056");
    let deployer = Address::ZERO;

    let ctx = Context::mainnet()
        .modify_cfg_chained(|cfg| cfg.spec = SpecId::LONDON)
        .with_db(CacheDB::new(EmptyDB::default()));

    let insp = EdgeCovInspector::new();
    let mut evm = ctx.build_mainnet_with_inspector(insp);

    // Just run a simple transaction to potentially capture some comparisons
    let _ = evm.inspect_tx(TxEnv {
        caller: deployer,
        gas_limit: 1000000,
        kind: TransactTo::Create,
        data: code.into(),
        ..Default::default()
    });

    // Reset and verify both hitcount and cmp_log are cleared
    evm.inspector().reset();
    assert!(evm.inspector().get_hitcount().iter().all(|&x| x == 0));
    assert!(evm.inspector().get_cmp_log().is_empty());
}

#[test]
fn test_cmp_operands_struct() {
    // Test CmpOperands struct
    let cmp = CmpOperands {
        op1: U256::from(123),
        op2: U256::from(456),
        pc: 42,
    };

    assert_eq!(cmp.op1, U256::from(123));
    assert_eq!(cmp.op2, U256::from(456));
    assert_eq!(cmp.pc, 42);

    // Test Default
    let default_cmp = CmpOperands::default();
    assert_eq!(default_cmp.op1, U256::ZERO);
    assert_eq!(default_cmp.op2, U256::ZERO);
    assert_eq!(default_cmp.pc, 0);

    // Test Clone
    let cloned = cmp.clone();
    assert_eq!(cloned.op1, cmp.op1);
    assert_eq!(cloned.op2, cmp.op2);
    assert_eq!(cloned.pc, cmp.pc);
}
