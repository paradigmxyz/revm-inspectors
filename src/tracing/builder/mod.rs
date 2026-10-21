//! Builder types for building traces

/// Geth style trace builders for `debug_` namespace
pub mod geth;

/// Parity style trace builders for `trace_` namespace
pub mod parity;

/// Whether a finalized account is deleted from state.
///
/// Before Cancun, selfdestructed accounts retain their deletion flag. From Cancun, revm
/// clears them in place and removes that flag, leaving an empty touched account for EIP-161
/// state clearing. Revm preserves pre-EIP-161 empty accounts by marking them created or
/// clearing their touched flag during finalization.
fn is_account_deleted(account: &revm::state::Account) -> bool {
    account.is_selfdestructed()
        || (account.is_touched() && account.is_empty() && !account.is_created())
}
