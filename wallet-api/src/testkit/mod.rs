//! Crate-side test kit for integration tests.
//!
//! `tests/harness` owns the external test environment. `testkit` exposes
//! intentional test-only entrypoints into internal wallet-api workflows.

pub mod env;

#[cfg(any(test, feature = "integration-tests"))]
pub mod expand;

#[cfg(any(test, feature = "integration-tests"))]
pub mod adapter_factory;
#[cfg(any(test, feature = "integration-tests"))]
pub mod api_wallet_resource;
#[cfg(any(test, feature = "integration-tests"))]
pub mod chain_sync;
#[cfg(any(test, feature = "integration-tests"))]
pub mod collect;
#[cfg(any(test, feature = "integration-tests"))]
pub mod collect_fee;
#[cfg(any(test, feature = "integration-tests"))]
pub mod context;
#[cfg(any(test, feature = "integration-tests"))]
pub mod mqtt;
#[cfg(any(test, feature = "integration-tests"))]
pub mod resource_reclaim;
#[cfg(any(test, feature = "integration-tests"))]
pub mod seed;
#[cfg(any(test, feature = "integration-tests"))]
pub mod sol_transaction;
#[cfg(any(test, feature = "integration-tests"))]
pub mod withdraw;

#[cfg(any(test, feature = "integration-tests"))]
pub mod ordinary_business;
