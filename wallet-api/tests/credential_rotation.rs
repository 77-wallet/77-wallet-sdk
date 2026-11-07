//! Standalone process: owns the global Manager credential lifecycle.
#![cfg(feature = "integration-tests")]
#[path = "integration/api_wallet/credential_rotation/mod.rs"]
mod credential_rotation;
#[path = "harness/mod.rs"]
mod harness;
