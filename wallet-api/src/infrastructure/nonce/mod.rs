// ⚠️ Production-stable nonce engine
// ⚠️ Concurrency model is intentional
// ⚠️ Do NOT parallelize reconcile
// ⚠️ Do NOT add global locks
// ⚠️ Modify only with production incident reference

pub mod nonce_bootstrap;
pub mod nonce_engine;
pub mod nonce_metrics;

pub(crate) mod transfer_gate;

#[cfg(test)]
mod transfer_gate_tests;
