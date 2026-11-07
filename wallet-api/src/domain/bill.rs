use crate::messaging::mqtt::topics::AcctChange;
use wallet_chain_interact::{BillResourceConsume, QueryTransactionResult};
use wallet_database::{
    CoreDbPool,
    entities::{
        self,
        asset_token_key::AssetTokenKey,
        bill::{BillEntity, BillKind, BillStatus},
        multisig_account::MultiAccountOwner,
    },
    repositories::{account::AccountRepo, bill::BillRepo, multisig_account::MultisigAccountRepo},
};
use wallet_transport_backend::response_vo::transaction::SyncBillResp;
use wallet_types::constant::chain_code;

pub struct BillDomain;

impl BillDomain {
    pub async fn create_bill<T>(
        pool: &CoreDbPool,
        params: entities::bill::NewBillEntity<T>,
    ) -> Result<(), crate::error::service::ServiceError>
    where
        T: serde::Serialize,
    {
        Ok(BillRepo::create(params, &pool).await?)
    }

    // 对于swap的交易，先判断有没有对应的交易
    pub async fn create_check_swap<T>(
        tx: entities::bill::NewBillEntity<T>,
        pool: &CoreDbPool,
    ) -> Result<(), crate::error::service::ServiceError>
    where
        T: serde::Serialize,
    {
        match BillRepo::get_by_hash_opt(&tx.hash, pool).await? {
            Some(bill) if bill.tx_kind == BillKind::Swap.to_i8() => {
                BillRepo::update_all(pool, tx, bill.id).await?;
            }
            _ => {
                BillRepo::create(tx, pool).await?;
            }
        }

        Ok(())
    }

    // query tx resource consume
    pub async fn get_bill_resource_consumer(
        ctx: &'static crate::context::Context,
        tx_hash: &str,
        chain_code: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let adapter = super::chain::adapter::ChainAdapterFactory::get_transaction_adapter_with_ctx(
            ctx, chain_code,
        )
        .await?;
        let res = adapter.query_tx_res(tx_hash).await?;
        match res {
            Some(res) => Ok(res.resource_consume),
            None => Ok("".to_string()),
        }
    }

    pub async fn get_onchain_bill(
        ctx: &'static crate::context::Context,
        tx_hash: &str,
        chain_code: &str,
    ) -> Result<Option<QueryTransactionResult>, crate::error::service::ServiceError> {
        let adapter = super::chain::adapter::ChainAdapterFactory::get_transaction_adapter_with_ctx(
            ctx, chain_code,
        )
        .await?;

        Ok(adapter.query_tx_res(tx_hash).await?)
    }

    pub async fn handle_sync_bill(
        pool: &CoreDbPool,
        item: SyncBillResp,
    ) -> Result<(), crate::error::service::ServiceError> {
        if item.value == 0.0 {
            return Ok(());
        }

        let status =
            if item.status { BillStatus::Success.to_i8() } else { BillStatus::Failed.to_i8() };

        let transaction_fee = item.transaction_fee();

        let mut new_entity = BillRepo::build_bill_with_extra::<serde_json::Value>(
            item.tx_hash,
            item.from_addr,
            item.to_addr,
            item.value,
            item.chain_code,
            item.symbol,
            item.is_multisig > 0,
            BillKind::try_from(item.tx_kind)?,
            item.notes,
        );
        new_entity.token = AssetTokenKey::from(item.token);
        new_entity.status = status;
        new_entity.transaction_fee = transaction_fee;
        new_entity.resource_consume = BillResourceConsume {
            net_used: item.net_used.unwrap_or_default(),
            energy_used: item.energy_used.unwrap_or_default(),
        }
        .to_json_str()?;
        new_entity.transaction_time =
            wallet_utils::time::datetime_to_timestamp(&item.transaction_time);
        new_entity.tx_type = item.transfer_type;
        new_entity.queue_id = item.queue_id.unwrap_or_default();
        new_entity.block_height = item.block_height.to_string();
        new_entity.signer = item.signer;
        new_entity.extra = item.extra;

        if new_entity.chain_code == chain_code::TON {
            let bill_pool = pool.read_pool();
            AcctChange::handle_ton_bill(new_entity, &bill_pool).await?;
        } else {
            BillDomain::create_check_swap(new_entity, pool).await?;
        }

        Ok(())
    }

    pub(crate) async fn sync_bills(
        pool: &CoreDbPool,
        backend: &wallet_transport_backend::api::BackendApi,
        chain_code: &str,
        address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let start_time = BillDomain::get_last_bill_time(pool, chain_code, address).await?;

        let resp = backend.record_lists(chain_code, address, start_time).await?;

        for item in resp.list {
            if let Err(e) = BillDomain::handle_sync_bill(pool, item).await {
                tracing::warn!("[bill::sync_bills] failed {}", e);
            }
        }

        Ok(())
    }

    pub(crate) fn handle_hash(hash: &str) -> String {
        match hash.split_once(':') {
            Some((before, _)) => before.to_owned(),
            None => hash.to_owned(),
        }
    }

    // For non-TRON networks, directly subtract 5 days from the time in the order.
    // For TRON network:
    // 1. Non-multisig accounts: the logic is the same as for non-TRON networks.
    // 2. Multisig accounts:
    //    1. If the account is a participant: only synchronize the order data created after the account was created.
    //    2. If the account is the creator: the logic is the same as for non-TRON networks.
    pub(crate) async fn get_last_bill_time(
        pool: &CoreDbPool,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<String>, crate::error::service::ServiceError> {
        let bill = BillRepo::last_bill(pool, chain_code, address)
            .await
            .map_err(|e| crate::error::service::ServiceError::Database(e.into()))?;

        let adjusted_time = |bill: Option<BillEntity>| {
            bill.map(|bill| {
                let time = bill.transaction_time - std::time::Duration::from_secs(86400 * 5);
                time.format("%Y-%m-%d %H:%M:%S").to_string()
            })
            .unwrap_or_else(|| sqlx::types::chrono::NaiveDateTime::default().to_string())
        };

        // Non-Tron chains
        if chain_code != wallet_types::constant::chain_code::TRON {
            return Ok(Some(adjusted_time(bill)));
        }

        // Tron-specific logic
        let account =
            AccountRepo::detail_by_address_and_chain_code(pool.clone(), address, chain_code)
                .await?;

        if account.is_some() {
            return Ok(Some(adjusted_time(bill)));
        }

        // Check multisig account if regular account not found
        let condition = vec![("address", address), ("chain_code", chain_code), ("is_del", "0")];
        let account = MultisigAccountRepo::find_by_conditions(pool, condition).await?;

        if let Some(account) = account {
            if account.owner == MultiAccountOwner::Participant.to_i8() {
                // If participant, compare bill and account creation time
                if let Some(bill) = bill {
                    let crate_time = account.created_at + std::time::Duration::from_secs(86400 * 5);
                    if bill.transaction_time > crate_time {
                        return Ok(Some(
                            bill.transaction_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                        ));
                    }
                }
                let time = account.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
                return Ok(Some(time));
            }
            return Ok(Some(adjusted_time(bill)));
        }

        Err(crate::error::business::BusinessError::MultisigAccount(
            crate::error::business::multisig_account::MultisigAccountError::NotFound,
        )
        .into())
    }
}
