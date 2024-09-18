use alloy_primitives::{address, b256, Address, Log, LogData, B256, U256};
use alloy_sol_types::SolValue;
use revm::{
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, CreateScheme, EOFCreateKind,
    },
    Database, EvmContext, Inspector, JournaledState,
};

/// Sender of ETH transfer log per `eth_simulateV1` spec.
///
/// <https://github.com/ethereum/execution-apis/pull/484>
pub const TRANSFER_LOG_EMITTER: Address = address!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");

/// Topic of `Transfer(address,address,uint256)` event.
pub const TRANSFER_EVENT_TOPIC: B256 =
    b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");

/// An [Inspector] that collects internal ETH transfers.
///
/// This can be used to construct `ots_getInternalOperations` or `eth_simulateV1` response.
#[derive(Debug, Default)]
pub struct TransferInspector {
    internal_only: bool,
    transfers: Vec<TransferOperation>,
    /// If enabled, will insert ERC20-style transfer logs emitted by [TRANSFER_LOG_EMITTER] for
    /// each ETH transfer.
    ///
    /// Can be used for [eth_simulateV1](https://github.com/ethereum/execution-apis/pull/484) execution.
    insert_logs: bool,
}

impl TransferInspector {
    /// Creates a new transfer inspector.
    ///
    /// If `internal_only` is set to `true`, only internal transfers are collected, in other words,
    /// the top level call is ignored.
    pub fn new(internal_only: bool) -> Self {
        Self { internal_only, transfers: Vec::new(), insert_logs: false }
    }

    /// Creates a new transfer inspector that only collects internal transfers.
    pub fn internal_only() -> Self {
        Self::new(true)
    }

    /// Consumes the inspector and returns the collected transfers.
    pub fn into_transfers(self) -> Vec<TransferOperation> {
        self.transfers
    }

    /// Sets whether to insert ERC20-style transfer logs.
    pub fn with_logs(mut self, insert_logs: bool) -> Self {
        self.insert_logs = insert_logs;
        self
    }

    /// Returns a reference to the collected transfers.
    pub fn transfers(&self) -> &[TransferOperation] {
        &self.transfers
    }

    /// Returns an iterator over the collected transfers.
    pub fn iter(&self) -> impl Iterator<Item = &TransferOperation> {
        self.transfers.iter()
    }

    fn on_transfer(
        &mut self,
        from: Address,
        to: Address,
        value: U256,
        kind: TransferKind,
        journaled_state: &mut JournaledState,
    ) {
        // skip top level transfers
        if self.internal_only && journaled_state.depth() == 0 {
            return;
        }
        // skip zero transfers
        if value.is_zero() {
            return;
        }
        self.transfers.push(TransferOperation { kind, from, to, value });

        if self.insert_logs {
            let from = B256::from_slice(&from.abi_encode());
            let to = B256::from_slice(&to.abi_encode());
            let data = value.abi_encode();

            journaled_state.log(Log {
                address: TRANSFER_LOG_EMITTER,
                data: LogData::new_unchecked(vec![TRANSFER_EVENT_TOPIC, from, to], data.into()),
            });
        }
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
        if let Some(value) = inputs.transfer_value() {
            self.on_transfer(
                inputs.transfer_from(),
                inputs.transfer_to(),
                value,
                TransferKind::Call,
                &mut context.journaled_state,
            );
        }

        None
    }

    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        let nonce = context.journaled_state.account(inputs.caller).info.nonce;
        let address = inputs.created_address(nonce);

        let kind = match inputs.scheme {
            CreateScheme::Create => TransferKind::Create,
            CreateScheme::Create2 { .. } => TransferKind::Create2,
        };

        self.on_transfer(inputs.caller, address, inputs.value, kind, &mut context.journaled_state);

        None
    }

    fn eofcreate(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut revm::interpreter::EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        let address = match inputs.kind {
            EOFCreateKind::Tx { .. } => {
                let nonce =
                    context.env.tx.nonce.unwrap_or_else(|| {
                        context.journaled_state.account(inputs.caller).info.nonce
                    });
                inputs.caller.create(nonce)
            }
            EOFCreateKind::Opcode { created_address, .. } => created_address,
        };

        self.on_transfer(
            inputs.caller,
            address,
            inputs.value,
            TransferKind::EofCreate,
            &mut context.journaled_state,
        );

        None
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
    /// A EOFCREATE operation
    EofCreate,
}
