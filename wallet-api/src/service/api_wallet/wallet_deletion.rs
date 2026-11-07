//! Bounded cleanup preparation for physical API-wallet deletion. The existing
//! task queue remains the durable retry authority; no new worker or schema.
use wallet_database::{
    entities::task_queue::{KnownTaskName, TaskName},
    repositories::{
        api_wallet::wallet::{ApiWalletDeletionRead, WalletDeletionSnapshot},
        task_queue::TaskQueueRepo,
    },
};
use wallet_transport_backend::request::{DeviceBindAddressReq, DeviceUnbindAddress};

use crate::{
    context::Context, error::service::ServiceError,
    infrastructure::task_queue::task::dispatch_task_entities_with_ctx,
};

const MAX_ADDRESSES: usize = 1000;
const MAX_REQUEST_BYTES: usize = 128 * 1024;

fn serialize_cleanup_request(request: &DeviceBindAddressReq) -> Result<String, ServiceError> {
    // Preserve BackendApiTaskData's persisted wire schema without allocating an
    // intermediate JSON object/map for every address during deletion.
    #[derive(serde::Serialize)]
    struct Body<'a> {
        endpoint: &'static str,
        body: &'a DeviceBindAddressReq,
    }
    Ok(wallet_utils::serde_func::serde_to_string(&Body {
        endpoint: wallet_transport_backend::consts::endpoint::DEVICE_UNBIND_ADDRESS,
        body: request,
    })?)
}

async fn persist_request(
    ctx: &'static Context,
    request: DeviceBindAddressReq,
    ids: &mut Vec<String>,
) -> Result<(), ServiceError> {
    if request.address.is_empty() {
        return Ok(());
    }
    let row = TaskQueueRepo::build_backend_task(
        TaskName::Known(KnownTaskName::BackendApi),
        Some(serialize_cleanup_request(&request)?),
        None,
    )?;
    let rows = TaskQueueRepo::create_multi_task(&ctx.task_pool()?, &[row]).await?;
    ids.extend(rows.into_iter().map(|row| row.id));
    Ok(())
}

pub(super) async fn persist_cleanup(
    ctx: &'static Context,
    read: &mut ApiWalletDeletionRead<'_>,
    addresses: &[&str],
) -> Result<(Vec<String>, WalletDeletionSnapshot), ServiceError> {
    let started = std::time::Instant::now();
    let mut read_time = std::time::Duration::ZERO;
    let mut hash_time = std::time::Duration::ZERO;
    let mut persist_time = std::time::Duration::ZERO;
    let sn = ctx.get_sn();
    let empty_bytes =
        wallet_utils::serde_func::serde_to_string(&DeviceBindAddressReq::new(sn))?.len();
    // All pages use the pre-backend SQLite snapshot. Its immutable view supplies
    // the original baseline without a separate full scan.
    let mut observed = read.empty_fingerprint();
    let mut cursor = 0;
    let mut ids = Vec::new();
    loop {
        let stage = std::time::Instant::now();
        let page = read.account_page(addresses, cursor).await?;
        read_time += stage.elapsed();
        let Some(last) = page.last() else { break };
        cursor = last.id;
        let stage = std::time::Instant::now();
        observed.include(&page);
        hash_time += stage.elapsed();
        let mut request = DeviceBindAddressReq::new(sn);
        let mut bytes = empty_bytes;
        for row in page {
            let entry = DeviceUnbindAddress { chain_code: row.chain_code, address: row.address };
            // Count serialized bytes, including escaping, commas and the envelope.
            let entry_bytes = wallet_utils::serde_func::serde_to_string(&entry)?.len();
            if empty_bytes + entry_bytes > MAX_REQUEST_BYTES {
                return Err(ServiceError::Parameter(
                    "API wallet cleanup address exceeds request limit".into(),
                ));
            }
            let separator = usize::from(!request.address.is_empty());
            if request.address.len() == MAX_ADDRESSES
                || bytes + separator + entry_bytes > MAX_REQUEST_BYTES
            {
                let stage = std::time::Instant::now();
                persist_request(ctx, request, &mut ids).await?;
                persist_time += stage.elapsed();
                request = DeviceBindAddressReq::new(sn);
                bytes = empty_bytes;
            }
            bytes += usize::from(!request.address.is_empty()) + entry_bytes;
            request.address.push(entry);
        }
        let stage = std::time::Instant::now();
        persist_request(ctx, request, &mut ids).await?;
        persist_time += stage.elapsed();
    }
    tracing::info!(target: "wallet_deletion_perf", stage="prepare_cleanup", rows=observed.len(), tasks=ids.len(),
        elapsed_ms=started.elapsed().as_millis() as u64, read_ms=read_time.as_millis() as u64,
        hash_ms=hash_time.as_millis() as u64, persist_ms=persist_time.as_millis() as u64);
    Ok((ids, observed))
}

pub(super) async fn dispatch_cleanup(
    ctx: &'static Context,
    ids: Vec<String>,
) -> Result<(), ServiceError> {
    let pool = ctx.task_pool()?;
    for id in ids {
        if let Some(row) = TaskQueueRepo::task_detail(&pool, &id).await? {
            dispatch_task_entities_with_ctx(ctx, vec![row]).await?;
        }
    }
    // A failure/cancellation here leaves persisted tasks recoverable after restart.
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn api_wallet_deletion_cleanup_wire_preserves_existing_consumer_schema() {
        use crate::infrastructure::task_queue::backend::BackendApiTaskData;
        let mut request =
            wallet_transport_backend::request::DeviceBindAddressReq::new("sn\"\\\n字");
        request.push("base", "plain");
        request.push("chain\"\\\n", "address\"\\\n字");
        let old = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_UNBIND_ADDRESS,
            &request,
        )
        .unwrap();
        let encoded = super::serialize_cleanup_request(&request).unwrap();
        let actual: BackendApiTaskData = serde_json::from_str(&encoded).unwrap();
        assert_eq!(actual.endpoint, old.endpoint);
        assert_eq!(actual.body, old.body);
    }
}
