use alloc::{vec, vec::Vec};
use alloy_primitives::{address, b256, Address, Log, LogData, B256, U256};
use revm::{
    context::JournalTr,
    context_interface::ContextTr,
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, CreateScheme},
    Database, Inspector,
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
/// This can be used to construct an `eth_simulateV1` response. For `ots_getInternalOperations`,
/// use [InternalOperationsInspector](crate::otterscan::InternalOperationsInspector).
#[derive(Debug, Default, Clone)]
pub struct TransferInspector {
    internal_only: bool,
    transfers: Vec<TransferOperation>,
    checkpoints: Vec<TransferCheckpoint>,
    log_mode: TransferLogMode,
    pending_journal_log: Option<Log>,
}

impl TransferInspector {
    /// Creates a new transfer inspector.
    ///
    /// If `internal_only` is set to `true`, only internal transfers are collected, in other words,
    /// the top level call is ignored.
    pub fn new(internal_only: bool) -> Self {
        Self { internal_only, ..Default::default() }
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
    ///
    /// This writes to the EVM journal and therefore affects receipts. For RPC-only logs, use
    /// [`Self::with_log_collection`] instead. Selecting either mode replaces the previous mode.
    pub fn with_logs(mut self, insert_logs: bool) -> Self {
        self.log_mode =
            if insert_logs { TransferLogMode::Journal } else { TransferLogMode::Disabled };
        self
    }

    /// Returns the collected transfers, excluding reverted or halted call frames.
    pub fn transfers(&self) -> &[TransferOperation] {
        &self.transfers
    }

    /// Returns an iterator over the collected transfers, excluding reverted or halted call frames.
    pub fn iter(&self) -> impl Iterator<Item = &TransferOperation> {
        self.transfers.iter()
    }

    fn on_transfer<DB: Database, JOURNAL: JournalTr<Database = DB>>(
        &mut self,
        from: Address,
        to: Address,
        value: U256,
        kind: TransferKind,
        journaled_state: &mut JOURNAL,
    ) {
        // skip top level transfers
        if self.internal_only && journaled_state.depth() == 0 {
            return;
        }
        if let Some(log) = self.record_transfer(TransferOperation { kind, from, to, value }) {
            journaled_state.log(log);
        }
    }

    /// Collects contract logs and synthetic transfer logs in execution order without writing
    /// synthetic logs to the EVM journal.
    ///
    /// The collected logs reset at the start of each transaction. Logs and transfers from reverted
    /// or halted call frames are discarded. This mode replaces journal insertion selected with
    /// [`Self::with_logs`]. Protocol transfer logs, if enabled by the EVM, are also collected;
    /// callers should disable synthetic transfer tracing after EIP-7708 activation to avoid
    /// duplicates.
    pub fn with_log_collection(mut self) -> Self {
        self.log_mode = TransferLogMode::Collect(Vec::new());
        self
    }

    /// Returns the current transaction's ordered logs, or an empty slice if collection is disabled.
    pub fn logs(&self) -> &[Log] {
        match &self.log_mode {
            TransferLogMode::Collect(logs) => logs,
            _ => &[],
        }
    }

    /// Takes the current transaction's ordered logs, leaving collection enabled for the next
    /// transaction. Returns an empty vector if collection is disabled.
    pub fn take_logs(&mut self) -> Vec<Log> {
        match &mut self.log_mode {
            TransferLogMode::Collect(logs) => core::mem::take(logs),
            _ => Vec::new(),
        }
    }

    fn enter_frame<CTX: ContextTr>(&mut self, context: &CTX) {
        let logs_len = match &mut self.log_mode {
            TransferLogMode::Collect(logs) => {
                if self.checkpoints.is_empty() {
                    logs.clear();
                }
                logs.len()
            }
            _ => 0,
        };
        self.checkpoints.push(TransferCheckpoint {
            transfers_len: self.transfers.len(),
            logs_len,
            journal_logs_len: context.journal().logs().len(),
        });
    }

    fn exit_frame<CTX: ContextTr>(&mut self, context: &mut CTX, success: bool) {
        let Some(checkpoint) = self.checkpoints.pop() else { return };
        if success {
            // The selfdestruct hook has no journal access and runs immediately before frame exit.
            if let Some(log) = self.pending_journal_log.take() {
                context.journal_mut().log(log);
            }
            return;
        }

        self.pending_journal_log = None;
        self.transfers.truncate(checkpoint.transfers_len);
        match &mut self.log_mode {
            TransferLogMode::Collect(logs) => logs.truncate(checkpoint.logs_len),
            TransferLogMode::Journal => {
                // Transfer logs are inserted before the EVM creates its frame checkpoint.
                let logs = context.journal_mut().take_logs();
                for log in logs.into_iter().take(checkpoint.journal_logs_len) {
                    context.journal_mut().log(log);
                }
            }
            TransferLogMode::Disabled => {}
        }
    }

    fn record_transfer(&mut self, transfer: TransferOperation) -> Option<Log> {
        if transfer.value.is_zero() {
            return None;
        }
        let log = match self.log_mode {
            TransferLogMode::Disabled => None,
            _ => Some(Log {
                address: TRANSFER_LOG_EMITTER,
                data: LogData::new_unchecked(
                    vec![TRANSFER_EVENT_TOPIC, transfer.from.into_word(), transfer.to.into_word()],
                    transfer.value.to_be_bytes::<32>().into(),
                ),
            }),
        };
        self.transfers.push(transfer);
        match (&mut self.log_mode, log) {
            (TransferLogMode::Collect(logs), Some(log)) => {
                logs.push(log);
                None
            }
            (_, log) => log,
        }
    }
}

#[derive(Debug, Default, Clone)]
enum TransferLogMode {
    #[default]
    Disabled,
    Journal,
    Collect(Vec<Log>),
}

#[derive(Debug, Clone)]
struct TransferCheckpoint {
    transfers_len: usize,
    logs_len: usize,
    journal_logs_len: usize,
}

impl<CTX> Inspector<CTX> for TransferInspector
where
    CTX: ContextTr,
{
    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.enter_frame(context);
        // CALLCODE checks the caller's balance but does not move ETH between accounts.
        if let Some(value) = inputs.transfer_value().filter(|_| inputs.scheme.is_call()) {
            self.on_transfer(
                inputs.transfer_from(),
                inputs.transfer_to(),
                value,
                TransferKind::Call,
                context.journal_mut(),
            );
        }

        None
    }

    fn call_end(&mut self, context: &mut CTX, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        self.exit_frame(context, outcome.instruction_result().is_ok());
    }

    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        self.enter_frame(context);
        let nonce = context.journal_mut().load_account(inputs.caller()).ok()?.data.info.nonce;
        let address = inputs.created_address(nonce);

        let kind = match inputs.scheme() {
            CreateScheme::Create => TransferKind::Create,
            CreateScheme::Create2 { .. } => TransferKind::Create2,
            CreateScheme::Custom { .. } => return None,
        };

        self.on_transfer(inputs.caller(), address, inputs.value(), kind, context.journal_mut());

        None
    }

    fn create_end(
        &mut self,
        context: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        self.exit_frame(context, outcome.instruction_result().is_ok());
    }

    fn log(&mut self, _context: &mut CTX, log: Log) {
        if let TransferLogMode::Collect(logs) = &mut self.log_mode {
            logs.push(log);
        }
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        self.pending_journal_log = self.record_transfer(TransferOperation {
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
    /// A non-zero value transfer CALL
    Call,
    /// A CREATE operation
    Create,
    /// A CREATE2 operation
    Create2,
    /// A SELFDESTRUCT operation
    SelfDestruct,
}
