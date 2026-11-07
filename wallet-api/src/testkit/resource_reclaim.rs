//! Test-only entrypoints for local resource reclaim scanner steps.

use wallet_database::ApiResourceDbPool;

use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::resource_reclaim::local_shadow::{
        LocalResourceReclaimIntent, LocalResourceReclaimScanner, LocalResourceReclaimScannerConfig,
    },
};

pub async fn scan_local_reclaim_intent_labels_once(
    collect_pool: ApiResourceDbPool,
) -> Result<Vec<String>, ServiceError> {
    let scanner = LocalResourceReclaimScanner::with_config(
        collect_pool,
        LocalResourceReclaimScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
        },
    );

    let mut labels = Vec::new();
    for intent in scanner.scan_round().await {
        let label = match intent {
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(_) => {
                "ExecuteLocalUndelegation".to_string()
            }
            LocalResourceReclaimIntent::RecoverLocalUndelegation(_) => {
                "RecoverLocalUndelegation".to_string()
            }
        };
        labels.push(label);
    }

    Ok(labels)
}
