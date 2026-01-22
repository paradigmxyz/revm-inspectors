//! Transaction reproduction test infrastructure.
//!
//! This module provides reusable tooling for replaying transactions with prestate data
//! captured from mainnet using the prestate tracer.
//!
//! # Example
//!
//! ```ignore
//! const FIXTURE: &str = include_str!("../../testdata/repro/my-fixture.json");
//!
//! #[test]
//! fn test_my_trace() {
//!     let ctx = ReproContext::load(FIXTURE);
//!     
//!     // Run with any tracer configuration
//!     let result = ctx.run_with_tracer(
//!         TracingInspectorConfig::default_geth(),
//!         |inspector, res, db| {
//!             let frame = inspector
//!                 .geth_builder()
//!                 .geth_call_traces(CallConfig::default(), res.result.gas_used());
//!             // assertions...
//!         },
//!     );
//! }
//! ```

use alloy_hardforks::{ethereum::mainnet::*, EthereumHardfork};
use alloy_primitives::{Address, Bytes, U256};
use alloy_rpc_types_trace::geth::{AccountState, PreStateConfig, PreStateFrame};
use revm::{
    bytecode::Bytecode, context::TxEnv, context_interface::TransactTo, database::CacheDB,
    database_interface::EmptyDB, primitives::hardfork::SpecId, state::AccountInfo, Context,
    InspectEvm, MainBuilder, MainContext,
};
use revm_inspectors::tracing::{TracingInspector, TracingInspectorConfig};
use serde::Deserialize;
use std::collections::BTreeMap;

/// A test fixture containing transaction and prestate data.
#[derive(Debug, Deserialize)]
pub struct ReproTestFixture {
    pub description: String,
    pub block_number: u64,
    pub transaction: TxData,
    pub prestate: BTreeMap<Address, AccountState>,
}

/// Transaction data from a fixture.
#[derive(Debug, Deserialize)]
pub struct TxData {
    pub from: Address,
    pub to: Option<Address>,
    pub input: Bytes,
    pub value: Option<U256>,
    pub gas: U256,
    pub nonce: U256,
}

/// Convert an Ethereum hardfork to a revm SpecId.
pub fn spec_id_from_ethereum_hardfork(hardfork: EthereumHardfork) -> SpecId {
    match hardfork {
        EthereumHardfork::Frontier => SpecId::FRONTIER,
        EthereumHardfork::Homestead => SpecId::HOMESTEAD,
        EthereumHardfork::Dao => SpecId::DAO_FORK,
        EthereumHardfork::Tangerine => SpecId::TANGERINE,
        EthereumHardfork::SpuriousDragon => SpecId::SPURIOUS_DRAGON,
        EthereumHardfork::Byzantium => SpecId::BYZANTIUM,
        EthereumHardfork::Constantinople => SpecId::CONSTANTINOPLE,
        EthereumHardfork::Petersburg => SpecId::PETERSBURG,
        EthereumHardfork::Istanbul => SpecId::ISTANBUL,
        EthereumHardfork::MuirGlacier => SpecId::MUIR_GLACIER,
        EthereumHardfork::Berlin => SpecId::BERLIN,
        EthereumHardfork::London => SpecId::LONDON,
        EthereumHardfork::ArrowGlacier => SpecId::ARROW_GLACIER,
        EthereumHardfork::GrayGlacier => SpecId::GRAY_GLACIER,
        EthereumHardfork::Paris => SpecId::MERGE,
        EthereumHardfork::Shanghai => SpecId::SHANGHAI,
        EthereumHardfork::Cancun => SpecId::CANCUN,
        EthereumHardfork::Prague => SpecId::PRAGUE,
        EthereumHardfork::Osaka => SpecId::OSAKA,
        _ => SpecId::PRAGUE,
    }
}

/// Determine the SpecId from a mainnet block number.
pub fn spec_id_from_block(block_number: u64) -> SpecId {
    let hardfork = hardfork_from_mainnet_block(block_number);
    spec_id_from_ethereum_hardfork(hardfork)
}

/// Determine the Ethereum hardfork active at a mainnet block number.
fn hardfork_from_mainnet_block(block_number: u64) -> EthereumHardfork {
    if block_number >= MAINNET_PRAGUE_BLOCK {
        EthereumHardfork::Prague
    } else if block_number >= MAINNET_CANCUN_BLOCK {
        EthereumHardfork::Cancun
    } else if block_number >= MAINNET_SHANGHAI_BLOCK {
        EthereumHardfork::Shanghai
    } else if block_number >= MAINNET_PARIS_BLOCK {
        EthereumHardfork::Paris
    } else if block_number >= MAINNET_GRAY_GLACIER_BLOCK {
        EthereumHardfork::GrayGlacier
    } else if block_number >= MAINNET_ARROW_GLACIER_BLOCK {
        EthereumHardfork::ArrowGlacier
    } else if block_number >= MAINNET_LONDON_BLOCK {
        EthereumHardfork::London
    } else if block_number >= MAINNET_BERLIN_BLOCK {
        EthereumHardfork::Berlin
    } else if block_number >= MAINNET_MUIR_GLACIER_BLOCK {
        EthereumHardfork::MuirGlacier
    } else if block_number >= MAINNET_ISTANBUL_BLOCK {
        EthereumHardfork::Istanbul
    } else if block_number >= MAINNET_PETERSBURG_BLOCK {
        EthereumHardfork::Petersburg
    } else if block_number >= MAINNET_BYZANTIUM_BLOCK {
        EthereumHardfork::Byzantium
    } else if block_number >= MAINNET_SPURIOUS_DRAGON_BLOCK {
        EthereumHardfork::SpuriousDragon
    } else if block_number >= MAINNET_TANGERINE_BLOCK {
        EthereumHardfork::Tangerine
    } else if block_number >= MAINNET_DAO_BLOCK {
        EthereumHardfork::Dao
    } else if block_number >= MAINNET_HOMESTEAD_BLOCK {
        EthereumHardfork::Homestead
    } else {
        EthereumHardfork::Frontier
    }
}

/// Build a CacheDB from prestate AccountState map.
pub fn build_db_from_prestate(prestate: &BTreeMap<Address, AccountState>) -> CacheDB<EmptyDB> {
    let mut db = CacheDB::new(EmptyDB::default());

    for (addr, state) in prestate {
        let balance = state.balance.unwrap_or_default();
        let nonce = state.nonce.unwrap_or_default();
        let code = state.code.as_ref().map(|c| Bytecode::new_raw(c.clone()));

        db.insert_account_info(
            *addr,
            AccountInfo {
                balance,
                nonce,
                code_hash: code.as_ref().map(|c| c.hash_slow()).unwrap_or_default(),
                code,
                ..Default::default()
            },
        );

        // Insert storage
        for (slot, value) in &state.storage {
            db.insert_account_storage(*addr, (*slot).into(), (*value).into()).unwrap();
        }
    }

    db
}

/// Context for replaying a transaction from a fixture.
#[derive(Debug)]
pub struct ReproContext {
    pub fixture: ReproTestFixture,
    pub spec_id: SpecId,
    pub db: CacheDB<EmptyDB>,
}

impl ReproContext {
    /// Load a ReproContext from a JSON fixture string.
    pub fn load(json: &str) -> Self {
        let fixture: ReproTestFixture = serde_json::from_str(json).expect("valid fixture");
        let spec_id = spec_id_from_block(fixture.block_number);
        let db = build_db_from_prestate(&fixture.prestate);

        Self { fixture, spec_id, db }
    }

    /// Load a ReproContext with a specific SpecId override.
    pub fn load_with_spec(json: &str, spec_id: SpecId) -> Self {
        let fixture: ReproTestFixture = serde_json::from_str(json).expect("valid fixture");
        let db = build_db_from_prestate(&fixture.prestate);

        Self { fixture, spec_id, db }
    }

    /// Create a TxEnv from the fixture's transaction data.
    pub fn tx_env(&self) -> TxEnv {
        let tx = &self.fixture.transaction;
        TxEnv {
            caller: tx.from,
            gas_limit: tx.gas.try_into().unwrap_or(u64::MAX),
            kind: tx.to.map(TransactTo::Call).unwrap_or(TransactTo::Create),
            data: tx.input.clone(),
            value: tx.value.unwrap_or_default(),
            nonce: tx.nonce.try_into().unwrap_or(0),
            ..Default::default()
        }
    }
}

const TX_SELFDESTRUCT: &str = include_str!("../../testdata/repro/tx-selfdestruct.json");

#[test]
fn test_prestate_tracer_selfdestruct() {
    let ctx = ReproContext::load(TX_SELFDESTRUCT);
    let prestate_config = PreStateConfig::default();

    let mut inspector =
        TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));

    let db = ctx.db.clone();
    let mut evm = Context::mainnet()
        .with_db(ctx.db.clone())
        .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
        .build_mainnet()
        .with_inspector(&mut inspector);

    let res = evm.inspect_tx(ctx.tx_env()).expect("tx should execute");
    assert!(res.result.is_success(), "tx failed: {:?}", res.result);

    // Get the prestate trace
    let frame = inspector
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_prestate_traces(&res, &prestate_config, db)
        .unwrap();

    // Verify the trace contains expected accounts
    match frame {
        PreStateFrame::Default(prestate) => {
            // The prestate should contain the accounts that were accessed
            assert!(prestate.0.contains_key(&ctx.fixture.transaction.from));
            if let Some(to) = ctx.fixture.transaction.to {
                assert!(prestate.0.contains_key(&to));
            }
        }
        PreStateFrame::Diff(_) => panic!("Expected default prestate, got diff"),
    }
}

#[test]
fn test_prestate_tracer_selfdestruct_diff_mode() {
    use alloy_rpc_types_trace::geth::DiffMode;

    let ctx = ReproContext::load(TX_SELFDESTRUCT);
    let prestate_config = PreStateConfig { diff_mode: Some(true), ..Default::default() };

    let mut inspector =
        TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));

    let db = ctx.db.clone();
    let mut evm = Context::mainnet()
        .with_db(ctx.db.clone())
        .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
        .build_mainnet()
        .with_inspector(&mut inspector);

    let res = evm.inspect_tx(ctx.tx_env()).expect("tx should execute");
    assert!(res.result.is_success(), "tx failed: {:?}", res.result);

    let frame = inspector
        .with_transaction_gas_used(res.result.gas_used())
        .geth_builder()
        .geth_prestate_traces(&res, &prestate_config, db)
        .unwrap();

    // In diff mode, we should see the changes between pre and post state
    match frame {
        PreStateFrame::Diff(DiffMode { pre, post }) => {
            // The sender should be in pre (balance changed)
            assert!(pre.contains_key(&ctx.fixture.transaction.from));
            // The sender should also be in post with updated state
            assert!(post.contains_key(&ctx.fixture.transaction.from));
        }
        PreStateFrame::Default(_) => panic!("Expected diff mode, got default"),
    }
}
