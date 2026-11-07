//! Test-only adapter override registry.
//!
//! Integration and crate-side tests use this module to replace transaction
//! adapters without reaching real chain RPCs.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::Arc;

pub use crate::domain::api_wallet::adapter::tx::ReplacementReceipt;

use crate::domain::api_wallet::adapter::tx::Tx;

static TEST_TRANSACTION_ADAPTER_OVERRIDE: Lazy<DashMap<String, Arc<dyn Tx + Send + Sync>>> =
    Lazy::new(DashMap::new);

pub fn set_test_transaction_adapter_override(chain_code: &str, adapter: Arc<dyn Tx + Send + Sync>) {
    TEST_TRANSACTION_ADAPTER_OVERRIDE.insert(chain_code.to_string(), adapter);
}

pub fn clear_test_transaction_adapter_override(chain_code: &str) {
    TEST_TRANSACTION_ADAPTER_OVERRIDE.remove(chain_code);
}

pub fn maybe_get_transaction_adapter_override(
    chain_code: &str,
) -> Option<Arc<dyn Tx + Send + Sync>> {
    TEST_TRANSACTION_ADAPTER_OVERRIDE.get(chain_code).map(|adapter| adapter.clone())
}
