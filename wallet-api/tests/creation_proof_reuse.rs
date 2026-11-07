//! Isolated Manager process: proof lifecycle assertions require a fresh inventory.
#![cfg(feature = "integration-tests")]
#[path = "integration/api_wallet/creation_proof_reuse.rs"]
mod creation_proof_reuse;
#[path = "harness/mod.rs"]
mod harness;
