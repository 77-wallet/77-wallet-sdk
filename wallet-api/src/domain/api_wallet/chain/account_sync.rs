//! Missing-chain work resumes from persisted account/init facts, not expansion batches.
use crate::{
    context::Context,
    domain::{
        api_wallet::{account::ApiAccountDomain, assets::ApiAssetsDomain, wallet::ApiWalletDomain},
        app::config::ConfigDomain,
    },
    error::{service::ServiceError, system::SystemError},
};
use wallet_database::{
    entities::api_account::ApiAccountEntity,
    repositories::api_wallet::{
        account::ApiAccountRepo, assets::ApiAssetsRepo, chain::ApiChainRepo, coin::ApiCoinRepo,
    },
};
use wallet_transport_backend::request::{
    AddressInitReq, TokenQueryPriceReq,
    api_wallet::address::{ApiAddressInitReq, ApiAddressInitReqBackend},
};

pub(crate) const SOURCE_PAGE_SIZE: u32 = 200;
pub(crate) const REGISTRATION_MAX_ITEMS: usize = 40;
/// Serialized backend address payload, before encryption/envelope overhead.
pub(crate) const REGISTRATION_MAX_BYTES: usize = 32 * 1024;
const OVERSIZED_ITEM: &str = "address registration item exceeds payload limit";

fn sync_error(message: &str) -> ServiceError {
    SystemError::Service(message.to_owned()).into()
}

pub(crate) async fn register_bounded(
    ctx: &'static Context,
    request: ApiAddressInitReq,
) -> Result<(), ServiceError> {
    let epoch = request.epoch;
    let batch_id = request.batch_id;
    let mut oversized_count = 0usize;
    let mut chunk = ApiAddressInitReq::new(epoch);
    chunk.batch_id = batch_id.clone();
    for address in request.address_list.0 {
        let mut singleton = ApiAddressInitReq::new(epoch);
        singleton.batch_id = batch_id.clone();
        singleton.address_list.add_address(address.clone());
        if payload_bytes(&singleton)? > REGISTRATION_MAX_BYTES {
            oversized_count += 1;
            continue;
        }
        chunk.address_list.add_address(address);
        if chunk.address_list.0.len() > REGISTRATION_MAX_ITEMS
            || payload_bytes(&chunk)? > REGISTRATION_MAX_BYTES
        {
            let last = chunk.address_list.0.pop().expect("just appended");
            flush(ctx, chunk).await?;
            chunk = ApiAddressInitReq::new(epoch);
            chunk.batch_id = batch_id.clone();
            chunk.address_list.add_address(last);
        }
    }
    flush(ctx, chunk).await?;
    if oversized_count > 0 {
        tracing::warn!(
            oversized_count,
            reason = "payload_limit",
            "A oversized items remain uninitialized; healthy chunks persisted"
        );
        Err(sync_error(OVERSIZED_ITEM))
    } else {
        Ok(())
    }
}
fn payload_bytes(request: &ApiAddressInitReq) -> Result<usize, ServiceError> {
    serde_json::to_vec(&ApiAddressInitReqBackend::from(request))
        .map(|body| body.len())
        .map_err(|_| sync_error("address registration serialization failed"))
}
async fn flush(ctx: &'static Context, request: ApiAddressInitReq) -> Result<(), ServiceError> {
    if request.address_list.0.is_empty() {
        return Ok(());
    }
    let address_count = request.address_list.0.len();
    let payload_size = payload_bytes(&request)?;
    let batch_id_hash = super::recovery::log_id(request.batch_id.as_deref().unwrap_or(""));
    crate::domain::api_wallet::chain::recovery::enqueue_registration(ctx, request).await?;
    tracing::info!(
        batch_id_hash,
        address_count,
        payload_size,
        "A bounded registration chunk persisted"
    );
    Ok(())
}
pub(crate) async fn run_chain(ctx: &'static Context, chain_code: &str) -> Result<(), ServiceError> {
    let _guard = ctx.api_chain_sync_lock.lock().await;
    let pool = ctx.api_wallet_pool()?;
    let through_id = ApiAccountRepo::account_high_watermark(&pool).await?;
    let epoch = ConfigDomain::get_keys_reset_epoch(ctx).await?;
    if epoch == 0 {
        return Err(sync_error("missing-chain sync requires valid reset epoch"));
    }
    tracing::info!(chain_code, epoch, through_id, "A reconcile started");
    let coins = ApiCoinRepo::coin_list(&pool).await?;
    let mut first_failure = None;
    for chain in ApiChainRepo::get_chain_list(&pool)
        .await?
        .into_iter()
        .filter(|chain| chain.chain_code == chain_code)
    {
        let mut after_id = 0;
        loop {
            let Some(session) = ctx.wallet_unlock_session_snapshot().await else {
                return Err(sync_error("A derivation deferred until wallet unlock"));
            };
            if session.is_expired() {
                return Err(sync_error("A derivation deferred until wallet unlock"));
            }
            if !ConfigDomain::check_epoch_validity(ctx, epoch).await? {
                return Ok(());
            }
            let Some(active_chain) = ApiChainRepo::detail(&pool, &chain.chain_code).await? else {
                break;
            };
            if active_chain.status != 1 {
                break;
            }
            let code: wallet_types::chain::chain::ChainCode =
                chain.chain_code.as_str().try_into()?;
            let types = crate::domain::wallet::WalletDomain::address_type_by_chain(code);
            let page = ApiAccountRepo::missing_chain_account_page_for_variants(
                &pool,
                &chain.chain_code,
                after_id,
                through_id,
                SOURCE_PAGE_SIZE,
                types.len() as u32,
            )
            .await?;
            if page.is_empty() {
                break;
            }
            after_id = page.last().expect("nonempty").id;
            let node = super::ApiChainDomain::get_node(&chain.chain_code, ctx).await?;
            let mut created = Vec::new();
            let mut source_failure_count = 0usize;
            let mut source_failure_reason = None;
            for source in &page {
                let result = async {
                    let mut derived = Vec::new();
                    let seed = zeroize::Zeroizing::new(
                        ApiWalletDomain::new(ctx).get_seed(&source.wallet_address).await?,
                    );
                    let index =
                        wallet_utils::address::AccountIndexMap::from_account_id(source.account_id)?;
                    for address_type in &types {
                        let instance: wallet_chain_instance::instance::ChainObject = (
                            &code,
                            address_type,
                            crate::domain::chain::ChainDomain::network_kind_from_node_network(
                                &node.network,
                            ),
                        )
                            .try_into()?;
                        let (_, account, _) = ApiAccountDomain::derive_subkey_fast_with_ctx(
                            ctx,
                            &source.uid,
                            &seed,
                            &source.wallet_address,
                            &index,
                            &instance,
                            &source.name,
                            false,
                            source.api_wallet_type,
                            false,
                        )
                        .await?;
                        derived.push(account);
                    }
                    Ok::<_, ServiceError>(derived)
                }
                .await;
                match result {
                    Ok(rows) => created.extend(rows),
                    Err(error @ ServiceError::Database(_)) => return Err(error),
                    Err(error) => {
                        source_failure_count += 1;
                        source_failure_reason = Some(super::recovery::log_reason(&error));
                        if first_failure.is_none() {
                            first_failure = Some(error);
                        }
                    }
                }
            }
            if !ConfigDomain::check_epoch_validity(ctx, epoch).await? {
                return first_failure.map_or(Ok(()), Err);
            }
            let created_count = created.len();
            ApiAccountRepo::upsert_account_multi(&pool, created).await?;
            let mut request = ApiAddressInitReq::new(epoch);
            let mut prices = TokenQueryPriceReq(Vec::new());
            let mut assets = Vec::new();
            for source in &page {
                // At most the known address variants for one logical account.
                let targets = ApiAccountRepo::target_uninitialized_for_logical(
                    &pool,
                    &source.wallet_address,
                    source.account_id,
                    source.api_wallet_type,
                    &chain.chain_code,
                    types.len() as u32 + 1,
                )
                .await?;
                if targets.len() > types.len() {
                    return Err(sync_error("unexpected address variants for logical account"));
                }
                for target in targets {
                    assets.extend(
                        ApiAssetsDomain::init_default_api_assets(
                            &coins,
                            &target.address,
                            &target.chain_code,
                            &mut prices,
                        )
                        .await?,
                    );
                    request.address_list.add_address(registration(ctx, &target));
                }
            }
            if !ConfigDomain::check_epoch_validity(ctx, epoch).await? {
                return first_failure.map_or(Ok(()), Err);
            }
            let asset_candidate_count = assets.len();
            let registration_count = request.address_list.0.len();
            ApiAssetsRepo::insert_missing_assets_multi(&pool, assets).await?;
            if let Err(error) = register_bounded(ctx, request).await {
                if matches!(&error,ServiceError::System(SystemError::Service(message)) if message==OVERSIZED_ITEM)
                {
                    if first_failure.is_none() {
                        first_failure = Some(error);
                    }
                } else {
                    return Err(error);
                }
            }
            tracing::info!(
                chain_code,
                epoch,
                after_id,
                through_id,
                source_count = page.len(),
                account_upsert_count = created_count,
                asset_candidate_count,
                registration_count,
                source_failure_count, source_failure_reason=?source_failure_reason,
                has_deferred_item = first_failure.is_some(),
                "A reconcile page persisted"
            );
            tokio::task::yield_now().await;
        }
    }
    tracing::info!(
        chain_code,
        epoch,
        has_deferred_item = first_failure.is_some(),
        "A reconcile scan finished; registration completion is separate"
    );
    first_failure.map_or(Ok(()), Err)
}
fn registration(ctx: &'static Context, row: &ApiAccountEntity) -> AddressInitReq {
    AddressInitReq::new(
        &row.uid,
        &row.address,
        row.derivation_path_index,
        &row.chain_code,
        ctx.get_sn(),
        vec!["".to_owned()],
        &row.name,
    )
}
