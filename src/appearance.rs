//! Inspector for collecting address appearances.
//!
//! This is based on the draft execution-apis address appearance work:
//! - <https://github.com/ethereum/execution-apis/pull/452>
//! - <https://github.com/ethereum/execution-apis/pull/453>
//! - <https://github.com/ethereum/execution-apis/pull/456>

use alloc::{collections::BTreeSet, vec::Vec};
use alloy_primitives::{hex, Address, Log, TxKind, B256, U256};
use revm::{
    bytecode::opcode,
    context::{
        transaction::{AccessListItemTr, AuthorizationTr},
        JournalTr,
    },
    context_interface::{ContextTr, Transaction},
    inspector::JournalExt,
    interpreter::{
        interpreter_types::Jumps, CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome,
        Interpreter,
    },
    Inspector,
};

/// An [Inspector] that collects addresses and address appearances.
///
/// The inspector records runtime address sources available during EVM execution, including calls,
/// creates, selfdestruct targets, logs, internal input/output data, and opcode address operands.
/// Callers can also feed block-level fields via [`record_block_field`](Self::record_block_field)
/// and set the transaction index before each transaction via
/// [`set_transaction_index`](Self::set_transaction_index).
///
/// If no transaction index is set, runtime addresses are still available through
/// [`addresses`](Self::addresses), but transaction appearances cannot be emitted because the
/// appearance location is unknown.
#[derive(Debug, Clone, Default)]
pub struct AppearanceInspector {
    addresses: BTreeSet<Address>,
    appearances: BTreeSet<AddressAppearance>,
    precompile_addresses: BTreeSet<Address>,
    current_location: Option<AppearanceLocation>,
    tx_fields_recorded: bool,
}

impl AppearanceInspector {
    /// Creates a new appearance inspector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the current transaction index.
    ///
    /// This should be called by block-level executors before each transaction when aggregating
    /// appearances for an entire block.
    pub fn set_transaction_index(&mut self, tx_index: usize) -> &mut Self {
        self.current_location = Some(AppearanceLocation::Transaction(tx_index));
        self.tx_fields_recorded = false;
        self
    }

    /// Clears the current transaction index.
    pub fn clear_transaction_index(&mut self) -> &mut Self {
        self.current_location = None;
        self.tx_fields_recorded = false;
        self
    }

    /// Clears all collected data and transaction state.
    pub fn clear(&mut self) {
        self.addresses.clear();
        self.appearances.clear();
        self.precompile_addresses.clear();
        self.current_location = None;
        self.tx_fields_recorded = false;
    }

    /// Returns all unique addresses collected so far.
    pub const fn addresses(&self) -> &BTreeSet<Address> {
        &self.addresses
    }

    /// Returns all unique appearances collected so far.
    pub const fn appearances(&self) -> &BTreeSet<AddressAppearance> {
        &self.appearances
    }

    /// Consumes the inspector and returns all unique addresses.
    pub fn into_addresses(self) -> BTreeSet<Address> {
        self.addresses
    }

    /// Consumes the inspector and returns all unique appearances.
    pub fn into_appearances(self) -> BTreeSet<AddressAppearance> {
        self.appearances
    }

    /// Records an address without an appearance location.
    pub fn record_address(&mut self, address: Address) -> bool {
        if self.is_precompile(address) {
            return false;
        }
        self.addresses.insert(address)
    }

    /// Records an address at a known appearance location.
    pub fn record_appearance(&mut self, address: Address, location: AppearanceLocation) -> bool {
        if self.is_precompile(address) {
            return false;
        }
        self.addresses.insert(address);
        self.appearances.insert(AddressAppearance::new(address, location))
    }

    /// Records a block-level address appearance.
    pub fn record_block_field(&mut self, address: Address, field: BlockField) -> bool {
        self.record_appearance(address, AppearanceLocation::BlockField(field))
    }

    /// Records all addresses that look like ABI-encoded address values in `bytes`.
    ///
    /// This follows the draft appearance classifier: data is right-aligned to a 32-byte boundary
    /// by skipping the leading `len % 32` bytes, then read in exact 32-byte chunks.
    pub fn record_potential_addresses_from_bytes(&mut self, bytes: &[u8]) {
        let location = self.current_location;
        for address in potential_addresses_from_bytes(bytes) {
            self.record_at_current_location(address, location);
        }
    }

    /// Records a log emitter, ABI-like addresses in log topics, and ABI-like addresses in log data.
    pub fn record_log(&mut self, log: &Log) {
        self.record_at_current_location(log.address, self.current_location);
        for topic in log.data.topics() {
            self.record_potential_address_word(topic);
        }
        self.record_potential_addresses_from_bytes(log.data.data.as_ref());
    }

    /// Records transaction body addresses from the current EVM context.
    pub fn record_transaction_fields<CTX>(&mut self, context: &CTX)
    where
        CTX: ContextTr<Journal: JournalExt>,
    {
        self.record_precompile_addresses(context);

        let tx = context.tx();
        let location = self.current_location;

        self.record_at_current_location(tx.caller(), location);
        if let TxKind::Call(to) = tx.kind() {
            self.record_at_current_location(to, location);
        }
        self.record_potential_addresses_from_bytes(tx.input());

        if let Some(access_list) = tx.access_list() {
            for item in access_list {
                self.record_at_current_location(*item.address(), location);
            }
        }

        for authorization in tx.authorization_list() {
            if let Some(authority) = authorization.authority() {
                self.record_at_current_location(authority, location);
            }
            self.record_at_current_location(authorization.address(), location);
        }
    }

    fn record_transaction_fields_once<CTX>(&mut self, context: &CTX)
    where
        CTX: ContextTr<Journal: JournalExt>,
    {
        if !self.tx_fields_recorded {
            self.record_transaction_fields(context);
            self.tx_fields_recorded = true;
        }
    }

    fn record_at_current_location(
        &mut self,
        address: Address,
        location: Option<AppearanceLocation>,
    ) -> bool {
        match location {
            Some(location) => self.record_appearance(address, location),
            None => self.record_address(address),
        }
    }

    fn record_potential_address_word(&mut self, word: &B256) {
        if let Some(address) = potential_address_word(word.as_slice()) {
            self.record_at_current_location(address, self.current_location);
        }
    }

    fn record_opcode_address_operand(&mut self, interp: &mut Interpreter, stack_index: usize) {
        if let Ok(word) = interp.stack.peek(stack_index) {
            self.record_at_current_location(
                Address::from_word(B256::from(word.to_be_bytes())),
                self.current_location,
            );
        }
    }

    fn record_precompile_addresses<CTX>(&mut self, context: &CTX)
    where
        CTX: ContextTr<Journal: JournalExt>,
    {
        self.precompile_addresses
            .extend(context.journal_ref().precompile_addresses().iter().copied());
    }

    fn is_precompile(&self, address: Address) -> bool {
        self.precompile_addresses.contains(&address)
    }
}

impl<CTX> Inspector<CTX> for AppearanceInspector
where
    CTX: ContextTr<Journal: JournalExt>,
{
    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        self.record_precompile_addresses(context);

        match interp.bytecode.opcode() {
            opcode::EXTCODECOPY
            | opcode::EXTCODEHASH
            | opcode::EXTCODESIZE
            | opcode::BALANCE
            | opcode::SELFDESTRUCT => self.record_opcode_address_operand(interp, 0),
            opcode::DELEGATECALL | opcode::CALL | opcode::STATICCALL | opcode::CALLCODE => {
                self.record_opcode_address_operand(interp, 1);
            }
            _ => {}
        }
    }

    fn log(&mut self, context: &mut CTX, log: Log) {
        self.record_precompile_addresses(context);
        self.record_log(&log);
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.record_transaction_fields_once(context);

        let (from, to) = match inputs.scheme {
            CallScheme::DelegateCall | CallScheme::CallCode => {
                (inputs.target_address, inputs.bytecode_address)
            }
            _ => (inputs.caller, inputs.target_address),
        };
        self.record_at_current_location(from, self.current_location);
        self.record_at_current_location(to, self.current_location);
        self.record_potential_addresses_from_bytes(&inputs.input.bytes_local(context.local()));

        None
    }

    fn call_end(&mut self, context: &mut CTX, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        self.record_precompile_addresses(context);
        self.record_potential_addresses_from_bytes(outcome.output());
    }

    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        self.record_transaction_fields_once(context);
        self.record_at_current_location(inputs.caller(), self.current_location);
        self.record_potential_addresses_from_bytes(inputs.init_code());

        None
    }

    fn create_end(
        &mut self,
        context: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        self.record_precompile_addresses(context);

        if let Some(address) = outcome.address {
            self.record_at_current_location(address, self.current_location);
        }
        self.record_potential_addresses_from_bytes(outcome.output());
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, _value: U256) {
        self.record_at_current_location(contract, self.current_location);
        self.record_at_current_location(target, self.current_location);
    }
}

/// Maximum number of leading or trailing zero hex characters allowed by the draft appearance
/// address classifier.
pub const MAX_VANITY_ZERO_CHARS: usize = 8;

/// The smallest number of non-zero bytes considered by the draft appearance address classifier.
pub const MIN_NONZERO_BYTES: usize = 20 - MAX_VANITY_ZERO_CHARS / 2;

const ADDRESS_WORD_PREFIX_LEN: usize = 12;
const ADDRESS_SUFFIX_ZERO_LEN: usize = MAX_VANITY_ZERO_CHARS / 2;
const SMALL_WORD: [u8; 32] =
    hex!("00000000000000000000000000000000000000ffffffffffffffffffffffffff");

/// A block-level field where an address appeared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlockField {
    /// The block beneficiary / miner field.
    Miner,
    /// An ommer / uncle beneficiary.
    Uncles,
    /// A withdrawal recipient.
    Withdrawals,
    /// A genesis allocation account.
    Alloc,
}

/// The location where an address appeared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AppearanceLocation {
    /// The address appeared in a transaction at this block-local index.
    Transaction(usize),
    /// The address appeared in a block-level field.
    BlockField(BlockField),
}

/// A de-duplicated address appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AddressAppearance {
    /// The address that appeared.
    pub address: Address,
    /// The location where the address appeared.
    pub location: AppearanceLocation,
}

impl AddressAppearance {
    /// Creates a new address appearance.
    #[inline]
    pub const fn new(address: Address, location: AppearanceLocation) -> Self {
        Self { address, location }
    }
}

/// Returns all potential ABI-encoded addresses in `bytes`.
///
/// This right-aligns `bytes` to a 32-byte boundary by skipping the leading `len % 32` bytes, then
/// reads exact 32-byte chunks.
pub fn potential_addresses_from_bytes(bytes: &[u8]) -> Vec<Address> {
    bytes[bytes.len() % 32..].chunks_exact(32).filter_map(potential_address_word).collect()
}

/// Returns the address encoded in a 32-byte word if the word matches the draft classifier.
pub fn potential_address_word(word: &[u8]) -> Option<Address> {
    if word.len() != 32 {
        return None;
    }
    if word <= SMALL_WORD.as_slice() {
        return None;
    }
    if word[..ADDRESS_WORD_PREFIX_LEN] != [0; ADDRESS_WORD_PREFIX_LEN] {
        return None;
    }

    let address = Address::from_slice(&word[ADDRESS_WORD_PREFIX_LEN..]);
    if address.as_slice()[20 - ADDRESS_SUFFIX_ZERO_LEN..].iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloy_primitives::{address, b256, bytes};

    #[test]
    fn potential_address_word_detects_padded_address() {
        let word = b256!("0000000000000000000000001111111111111111111111111111111111111111");
        assert_eq!(
            potential_address_word(word.as_slice()),
            Some(address!("1111111111111111111111111111111111111111"))
        );
    }

    #[test]
    fn potential_address_word_rejects_small_and_right_padded_values() {
        let small = b256!("0000000000000000000000000000000000000000000000000000000000000001");
        let right_padded =
            b256!("0000000000000000000000001111111111111111111111111111111100000000");

        assert_eq!(potential_address_word(small.as_slice()), None);
        assert_eq!(potential_address_word(right_padded.as_slice()), None);
    }

    #[test]
    fn potential_addresses_from_bytes_skips_leading_remainder() {
        let bytes = bytes!(
            "aabbccdd\
             0000000000000000000000001111111111111111111111111111111111111111"
        );
        assert_eq!(
            potential_addresses_from_bytes(&bytes),
            vec![address!("1111111111111111111111111111111111111111")]
        );
    }

    #[test]
    fn potential_addresses_from_bytes_rejects_cross_boundary_address() {
        let bytes = bytes!(
            "0000000000000000000000001111111111111111111111111111111111111111\
             00000000"
        );
        assert!(potential_addresses_from_bytes(&bytes).is_empty());
    }
}
