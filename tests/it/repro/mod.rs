//! Transaction reproduction test infrastructure.
//!
//! This module provides reusable tooling for replaying transactions with prestate data
//! captured from mainnet using the prestate tracer.
//!
//! The prestate JSON fixture should be the raw RPC response from `debug_traceCall` or
//! `debug_traceTransaction` with the prestate tracer. Transaction data is provided
//! separately as a constructed `TxEnv`.
//!
//! # Example
//!
//! ```ignore
//! use crate::repro::ReproContext;
//!
//! // Raw prestate tracer RPC response (copy-paste from RPC)
//! const PRESTATE: &str = include_str!("../../../testdata/repro/my-prestate.json");
//!
//! #[test]
//! fn test_my_trace() {
//!     let ctx = ReproContext::from_prestate_response(PRESTATE)
//!         .with_block_number(19660754); // or .with_spec_id(SpecId::CANCUN)
//!
//!     // Construct TxEnv from transaction data
//!     let tx_env = TxEnv {
//!         caller: address!("..."),
//!         kind: TransactTo::Call(address!("...")),
//!         data: hex!("...").into(),
//!         nonce: 123,
//!         gas_limit: 150000,
//!         ..Default::default()
//!     };
//!
//!     let mut inspector = TracingInspector::new(
//!         TracingInspectorConfig::from_geth_prestate_config(&PreStateConfig::default())
//!     );
//!
//!     let mut evm = Context::mainnet()
//!         .with_db(ctx.db.clone())
//!         .modify_cfg_chained(|cfg| cfg.spec = ctx.spec_id)
//!         .build_mainnet()
//!         .with_inspector(&mut inspector);
//!
//!     let res = evm.inspect_tx(tx_env).unwrap();
//!     // ... assertions on trace results
//! }
//! ```

mod prestate;

use alloy_hardforks::{ethereum::mainnet::*, EthereumHardfork};
use alloy_primitives::Address;
use alloy_rpc_types_trace::geth::AccountState;
use revm::{
    bytecode::Bytecode, database::CacheDB, database_interface::EmptyDB,
    primitives::hardfork::SpecId, state::AccountInfo,
};
use serde::Deserialize;
use std::collections::BTreeMap;

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

/// Wrapper for parsing raw prestate tracer RPC response.
///
/// Handles both direct prestate maps and JSON-RPC wrapped responses.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PrestateResponse {
    /// Direct prestate map (e.g., from `result` field)
    Direct(BTreeMap<Address, AccountState>),
    /// JSON-RPC wrapped response
    Wrapped { result: BTreeMap<Address, AccountState> },
}

impl PrestateResponse {
    fn into_prestate(self) -> BTreeMap<Address, AccountState> {
        match self {
            Self::Direct(prestate) => prestate,
            Self::Wrapped { result } => result,
        }
    }
}

/// Context for replaying a transaction with prestate data.
///
/// The prestate is loaded from a JSON fixture (raw RPC response format).
/// Transaction data is provided separately via RLP bytes or constructed `TxEnv`.
#[derive(Debug, Clone)]
pub struct ReproContext {
    /// The prestate accounts loaded from the fixture.
    pub prestate: BTreeMap<Address, AccountState>,
    /// The EVM spec to use for execution.
    pub spec_id: SpecId,
    /// The database populated with prestate.
    pub db: CacheDB<EmptyDB>,
}

impl ReproContext {
    /// Create a ReproContext from a raw prestate tracer RPC response.
    ///
    /// Accepts both the raw `result` field content or the full JSON-RPC response.
    ///
    /// # Example
    /// ```ignore
    /// // Direct prestate map
    /// let ctx = ReproContext::from_prestate_response(r#"{"0x1234...": {"balance": "0x0"}}"#);
    ///
    /// // Or full JSON-RPC response
    /// let ctx = ReproContext::from_prestate_response(r#"{"jsonrpc":"2.0","id":1,"result":{...}}"#);
    /// ```
    pub fn from_prestate_response(json: &str) -> Self {
        let response: PrestateResponse = serde_json::from_str(json).expect("valid prestate JSON");
        let prestate = response.into_prestate();
        let db = build_db_from_prestate(&prestate);

        Self { prestate, spec_id: SpecId::PRAGUE, db }
    }

    /// Set the spec ID (hardfork) for EVM execution.
    #[must_use]
    pub fn with_spec_id(mut self, spec_id: SpecId) -> Self {
        self.spec_id = spec_id;
        self
    }

    /// Set the spec ID based on a mainnet block number.
    #[must_use]
    pub fn with_block_number(mut self, block_number: u64) -> Self {
        self.spec_id = spec_id_from_block(block_number);
        self
    }
}
