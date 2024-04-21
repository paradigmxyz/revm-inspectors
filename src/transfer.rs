use alloy_primitives::{Address, U256};
use revm::{
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, CreateScheme},
    Database, EvmContext, Inspector,
};

/// An [Inspector] that collects internal ETH transfers.
///
/// This can be used to construct via `ots_getInternalOperations`
#[derive(Debug, Default)]
pub struct TransferInspector {
    internal_only: bool,
    transfers: Vec<TransferOperation>,
}

impl TransferInspector {
    /// Creates a new transfer inspector.
    ///
    /// If `internal_only` is set to `true`, only internal transfers are collected, in other words,
    /// the top level call is ignored.
    pub fn new(internal_only: bool) -> Self {
        Self { internal_only, transfers: Vec::new() }
    }

    /// Creates a new transfer inspector that only collects internal transfers.
    pub fn internal_only() -> Self {
        Self::new(true)
    }

    /// Consumes the inspector and returns the collected transfers.
    pub fn into_transfers(self) -> Vec<TransferOperation> {
        self.transfers
    }

    /// Returns a reference to the collected transfers.
    pub fn transfers(&self) -> &[TransferOperation] {
        &self.transfers
    }

    /// Returns an iterator over the collected transfers.
    pub fn iter(&self) -> impl Iterator<Item = &TransferOperation> {
        self.transfers.iter()
    }
}

impl<DB> Inspector<DB> for TransferInspector
where
    DB: Database,
{
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        if self.internal_only && context.journaled_state.depth() == 0 {
            // skip top level call
            return None;
        }

        if inputs.transfers_value() {
            self.transfers.push(TransferOperation {
                kind: TransferKind::Call,
                from: inputs.caller,
                to: inputs.target_address,
                value: inputs.call_value(),
            });
        }

        None
    }

    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        if self.internal_only && context.journaled_state.depth() == 0 {
            return outcome;
        }
        if let Some(address) = outcome.address {
            let kind = match inputs.scheme {
                CreateScheme::Create => TransferKind::Create,
                CreateScheme::Create2 { .. } => TransferKind::Create2,
            };
            self.transfers.push(TransferOperation {
                kind,
                from: inputs.caller,
                to: address,
                value: inputs.value,
            });
        }
        outcome
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        self.transfers.push(TransferOperation {
            kind: TransferKind::SelfDestruct,
            from: contract,
            to: target,
            value,
        });
    }
}

/// A transfer operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOperation {
    /// Source of the transfer call.
    pub kind: TransferKind,
    /// Sender of the transfer.
    pub from: Address,
    /// Receiver of the transfer.
    pub to: Address,
    /// Value of the transfer.
    pub value: U256,
}

/// The kind of transfer operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferKind {
    /// A non zero value transfer CALL
    Call,
    /// A CREATE operation
    Create,
    /// A CREATE2 operation
    Create2,
    /// A SELFDESTRUCT operation
    SelfDestruct,
}
