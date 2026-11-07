//! Isolated process: shared test inventories may retain unrelated addresses.
#![cfg(feature = "integration-tests")]
#[path = "harness/mod.rs"]
mod harness;
#[path = "integration/api_wallet/wallet_deletion_exclusive.rs"]
mod wallet_deletion_exclusive;
