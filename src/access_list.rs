use alloc::collections::BTreeSet;
use alloy_primitives::{
    map::{HashMap, HashSet},
    Address, TxKind, B256,
};
use alloy_rpc_types_eth::{AccessList, AccessListItem};
use revm::{
    interpreter::{opcode, Interpreter},
    Database, EvmContext, Inspector,
};

/// An [Inspector] that collects touched accounts and storage slots.
///
/// This can be used to construct an [AccessList] for a transaction via `eth_createAccessList`
#[derive(Debug, Default)]
pub struct AccessListInspector {
    /// All addresses that should be excluded from the final accesslist
    excluded: HashSet<Address>,
    /// All addresses and touched slots
    access_list: HashMap<Address, BTreeSet<B256>>,
}

impl AccessListInspector {
    /// Creates a new inspector instance
    ///
    /// The `access_list` is the provided access list from the call request
    pub fn new(access_list: AccessList) -> Self {
        Self {
            excluded: Default::default(),
            access_list: access_list
                .0
                .into_iter()
                .map(|v| (v.address, v.storage_keys.into_iter().collect()))
                .collect(),
        }
    }

    /// Returns list of addresses and storage keys used by the transaction. It gives you the list of
    /// addresses and storage keys that were touched during execution.
    pub fn into_access_list(self) -> AccessList {
        let items = self.access_list.into_iter().map(|(address, slots)| AccessListItem {
            address,
            storage_keys: slots.into_iter().collect(),
        });
        AccessList(items.collect())
    }

    /// Returns list of addresses and storage keys used by the transaction. It gives you the list of
    /// addresses and storage keys that were touched during execution.
    pub fn access_list(&self) -> AccessList {
        let items = self.access_list.iter().map(|(address, slots)| AccessListItem {
            address: *address,
            storage_keys: slots.iter().copied().collect(),
        });
        AccessList(items.collect())
    }

    /// Collects addresses which should be excluded from the access list.
    ///
    /// Those include caller, callee and precompiles.
    fn collect_excluded_addresses<DB: Database>(&mut self, context: &EvmContext<DB>) {
        let from = context.env.tx.caller;
        let to = if let TxKind::Call(to) = context.env.tx.transact_to {
            to
        } else {
            let nonce = context.journaled_state.account(from).info.nonce;
            from.create(nonce)
        };
        let precompiles = context.precompiles.addresses().copied();
        self.excluded = [from, to].into_iter().chain(precompiles).collect();
    }
}

impl<DB> Inspector<DB> for AccessListInspector
where
    DB: Database,
{
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &mut revm::interpreter::CallInputs,
    ) -> Option<revm::interpreter::CallOutcome> {
        // At the top-level frame, fill the excluded addresses
        if context.journaled_state.depth() == 0 {
            self.collect_excluded_addresses(context)
        }
        None
    }

    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &mut revm::interpreter::CreateInputs,
    ) -> Option<revm::interpreter::CreateOutcome> {
        // At the top-level frame, fill the excluded addresses
        if context.journaled_state.depth() == 0 {
            self.collect_excluded_addresses(context)
        }
        None
    }

    fn eofcreate(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &mut revm::interpreter::EOFCreateInputs,
    ) -> Option<revm::interpreter::CreateOutcome> {
        // At the top-level frame, fill the excluded addresses
        if context.journaled_state.depth() == 0 {
            self.collect_excluded_addresses(context)
        }
        None
    }

    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        match interp.current_opcode() {
            opcode::SLOAD | opcode::SSTORE => {
                if let Ok(slot) = interp.stack().peek(0) {
                    let cur_contract = interp.contract.target_address;
                    self.access_list
                        .entry(cur_contract)
                        .or_default()
                        .insert(B256::from(slot.to_be_bytes()));
                }
            }
            opcode::EXTCODECOPY
            | opcode::EXTCODEHASH
            | opcode::EXTCODESIZE
            | opcode::BALANCE
            | opcode::SELFDESTRUCT => {
                if let Ok(slot) = interp.stack().peek(0) {
                    let addr = Address::from_word(B256::from(slot.to_be_bytes()));
                    if !self.excluded.contains(&addr) {
                        self.access_list.entry(addr).or_default();
                    }
                }
            }
            opcode::DELEGATECALL | opcode::CALL | opcode::STATICCALL | opcode::CALLCODE => {
                if let Ok(slot) = interp.stack().peek(1) {
                    let addr = Address::from_word(B256::from(slot.to_be_bytes()));
                    if !self.excluded.contains(&addr) {
                        self.access_list.entry(addr).or_default();
                    }
                }
            }
            _ => (),
        }
    }
}
