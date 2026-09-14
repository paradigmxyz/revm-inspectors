//! Otterscan internal operations inspector.

use alloc::vec::Vec;
use alloy_primitives::{Address, U256};
use alloy_rpc_types_trace::otterscan::{InternalOperation, OperationType};
use revm::{
    context::JournalTr,
    context_interface::ContextTr,
    interpreter::{CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome, CreateScheme},
    Inspector,
};

/// Records attempted internal operations for Otterscan's `ots_getInternalOperations`.
///
/// Collects non-zero-value `CALL` transfers, `CREATE`, `CREATE2`, and `SELFDESTRUCT`
/// operations in execution order without retaining call inputs or outputs. Top-level calls and
/// creations are excluded, while zero-value creations and self-destructs are included.
///
/// Operations are retained even if their frame or an ancestor reverts. This is an execution
/// history, not a list of committed balance changes.
#[derive(Debug, Default, Clone)]
pub struct InternalOperationsInspector {
    operations: Vec<InternalOperation>,
}

impl InternalOperationsInspector {
    /// Returns the collected operations in execution order.
    pub fn operations(&self) -> &[InternalOperation] {
        &self.operations
    }

    /// Consumes the inspector and returns the collected operations in execution order.
    pub fn into_operations(self) -> Vec<InternalOperation> {
        self.operations
    }
}

impl<CTX: ContextTr> Inspector<CTX> for InternalOperationsInspector {
    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        if context.journal().depth() == 0 || inputs.scheme != CallScheme::Call {
            return None;
        }
        if let Some(value) = inputs.transfer_value().filter(|value| !value.is_zero()) {
            self.operations.push(InternalOperation {
                from: inputs.transfer_from(),
                to: inputs.transfer_to(),
                value,
                r#type: OperationType::OpTransfer,
            });
        }
        None
    }

    fn create(&mut self, context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        if context.journal().depth() == 0 {
            return None;
        }
        let r#type = match inputs.scheme() {
            CreateScheme::Create => OperationType::OpCreate,
            CreateScheme::Create2 { .. } => OperationType::OpCreate2,
            CreateScheme::Custom { .. } => return None,
        };
        let nonce = context.journal_mut().load_account(inputs.caller()).ok()?.data.info.nonce;
        self.operations.push(InternalOperation {
            from: inputs.caller(),
            to: inputs.created_address(nonce),
            value: inputs.value(),
            r#type,
        });
        None
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        self.operations.push(InternalOperation {
            from: contract,
            to: target,
            value,
            r#type: OperationType::OpSelfDestruct,
        });
    }
}
