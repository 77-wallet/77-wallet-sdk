use crate::{
    application::api_wallet_transfer::ApiTransferApplication,
    context::Context,
    domain::{
        api_wallet::{
            adapter::tx::RawTx, adapter_factory::ApiChainAdapterFactory, coin::ApiCoinDomain,
        },
        app::config::ConfigDomain,
        bill::BillDomain,
    },
    error::{business::api_wallet::ApiWalletError, service::ServiceError},
    request::api_wallet::transfer::ApiTransferExReq,
    response_vo::{
        api_wallet::transaction::{
            ApiBillDetailVo, ApiWalletResourceConsume, api_wallet_resource_consume,
        },
        standard_wallet::transaction::TransactionResult,
    },
};
use chrono::Utc;
use futures::future::join_all;
use std::collections::HashSet;
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_database::{
    ApiTransactionDbPool, ApiWalletDbPool, CoreDbPool,
    entities::{
        api_account::ApiAccountEntity,
        api_resource_operation::{ApiResourceOperationEntity, ApiResourceOperationType},
        api_resource_type::ApiResourceType,
        api_trade_type::ApiTradeType,
        api_wallet::ApiWalletEntity,
        api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus},
        asset_token_key::AssetTokenKey,
        bill::{BillEntity, BillKind, RecentBillListVo, SyncBillEntity},
    },
    pagination::Pagination,
    repositories::{
        api_wallet::{
            account::ApiAccountRepo, coin::ApiCoinRepo,
            resource_operation::ApiResourceOperationRepo, wallet::ApiWalletRepo,
            withdraw::ApiWithdrawRepo,
        },
        bill::BillRepo,
    },
};
use wallet_utils::unit;

pub struct ApiTransService {
    ctx: &'static Context,
}

impl ApiTransService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn transfer(
        &self,
        params: ApiTransferExReq,
        _bill_kind: BillKind,
    ) -> Result<TransactionResult, ServiceError> {
        ApiTransferApplication::new(self.ctx).transfer(params).await
    }

    pub(crate) async fn transfer_with_private_key(
        &self,
        params: ApiTransferExReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransactionResult, ServiceError> {
        ApiTransferApplication::new(self.ctx).transfer_with_private_key(params, private_key).await
    }

    pub async fn bill_detail(
        &self,
        tx_hash: &str,
        owner: &str,
    ) -> Result<ApiBillDetailVo, ServiceError> {
        let tx_hash = BillDomain::handle_hash(tx_hash);

        let api_transaction_pool = self.ctx.api_transaction_pool()?;
        let core_pool = self.ctx.api_wallet_pool()?;
        let bill =
            match ApiWithdrawRepo::get_by_hash_and_owner(&api_transaction_pool, owner, &tx_hash)
                .await
            {
                Ok(bill) => bill,
                Err(err) if Self::is_bill_detail_primary_lookup_not_found(&err) => {
                    let api_resource_pool = self.ctx.api_resource_pool()?;
                    return Self::resource_operation_bill_detail_by_hash(
                        &api_resource_pool,
                        owner,
                        &tx_hash,
                    )
                    .await;
                }
                Err(err) => return Err(err.into()),
            };

        let main_coin = ApiCoinRepo::main_coin(&bill.chain_code, &core_pool).await?;
        let resource_consume = api_wallet_resource_consume(&bill.resource_consume);
        let transfer_type = Self::default_transfer_type_by_trade_type(bill.trade_type);
        let e = self.convert_to_bill_entity(&bill, transfer_type);
        Ok(ApiBillDetailVo {
            bill: e,
            resource_consume,
            fee_symbol: main_coin.symbol.to_string(),
            signature: None,
            wallet_name: "".to_string(),
            account_name: "".to_string(),
        })
    }

    pub(crate) async fn resource_operation_bill_detail_by_hash(
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        owner: &str,
        tx_hash: &str,
    ) -> Result<ApiBillDetailVo, ServiceError> {
        let operation =
            ApiResourceOperationRepo::get_by_hash_and_owner(api_resource_pool, tx_hash, owner)
                .await?
                .ok_or(wallet_database::Error::NotFound(format!(
                    "api resource operation not found,tx_hash = {} ,owner = {}",
                    tx_hash, owner,
                )))?;
        let resource_consume = Self::operation_resource_consume(&operation)?;
        let bill = Self::convert_resource_operation_to_bill_entity(&operation);
        Ok(ApiBillDetailVo {
            bill,
            resource_consume,
            fee_symbol: "TRX".to_string(),
            signature: None,
            wallet_name: "".to_string(),
            account_name: "".to_string(),
        })
    }

    fn is_bill_detail_primary_lookup_not_found(err: &wallet_database::Error) -> bool {
        matches!(err, wallet_database::Error::NotFound(_))
            || matches!(
                err,
                wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
                    sqlx::Error::RowNotFound
                ))
            )
    }

    pub async fn list_by_hashs(
        &self,
        tx_hash: Vec<String>,
        owner: &str,
    ) -> Result<Vec<BillEntity>, crate::error::service::ServiceError> {
        let api_transaction_pool = self.ctx.api_transaction_pool()?;
        let bills = ApiWithdrawRepo::lists_by_hashs(&api_transaction_pool, owner, tx_hash).await?;

        let futures = bills.iter().map(|bill| async move {
            let transfer_type = Self::default_transfer_type_by_trade_type(bill.trade_type);
            let e = self.convert_to_bill_entity(&bill, transfer_type);
            Ok(e)
        });
        let results: Vec<Result<BillEntity, ServiceError>> = join_all(futures).await;
        let results: Result<Vec<BillEntity>, ServiceError> = results.into_iter().collect();

        Ok(results?)
    }

    pub async fn bill_lists(
        &self,
        root_addr: Option<String>,
        account_id: Option<u32>,
        addr: Option<String>,
        chain_code: Option<&str>,
        symbol: Option<&str>,
        is_multisig: Option<i64>,
        filter_min_value: Option<bool>,
        start: Option<i64>,
        end: Option<i64>,
        tx_kind: Vec<i32>,
        transfer_type: Option<i32>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<BillEntity>, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let api_transaction_pool = self.ctx.api_transaction_pool()?;
        let uid = match root_addr.clone() {
            Some(addr) => {
                let wallet: ApiWalletEntity = ApiWalletRepo::find_by_address(&pool, addr.as_str())
                    .await?
                    .ok_or(ServiceError::Business(
                        ApiWalletError::Wallet(
                            crate::error::business::api_wallet::wallet::WalletError::NotFound
                                .into(),
                        )
                        .into(),
                    ))?;
                wallet
            }
            None => {
                return Err(ServiceError::Business(
                    ApiWalletError::Wallet(
                        crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                    )
                    .into(),
                ));
            }
        };
        let reference_addrs = self
            .build_reference_addrs(&pool, addr, root_addr.clone(), account_id, chain_code)
            .await?;
        let masked_reference_addrs =
            reference_addrs.iter().map(|item| Self::mask_addr(item)).collect::<Vec<_>>();
        tracing::info!(
            reference_addr_count = reference_addrs.len(),
            reference_addrs = ?masked_reference_addrs,
            account_id = ?account_id,
            "api_bill_lists resolved reference addresses"
        );

        // 过滤最小金额
        let min_value = match (symbol, filter_min_value) {
            (Some(symbol), Some(true)) => {
                ConfigDomain::get_config_min_value(self.ctx, symbol).await?
            }
            _ => None,
        };
        let mut tx_kinds = vec![];
        for tx in tx_kind {
            if tx == BillKind::Transfer as i32 {
                if let Some(transfer) = transfer_type {
                    if transfer == 0 {
                        tx_kinds.push(ApiTradeType::SelfRecharge as i32);
                    } else if transfer == 1 {
                        tx_kinds.push(ApiTradeType::SelfWithdraw as i32);
                    }
                } else {
                    tx_kinds.push(ApiTradeType::SelfWithdraw as i32);
                    tx_kinds.push(ApiTradeType::SelfRecharge as i32);
                }
            } else if tx == BillKind::ApiWithdraw as i32 {
                tx_kinds.push(ApiTradeType::Withdraw as i32);
            }
        }

        let res = ApiWithdrawRepo::bill_lists(
            &api_transaction_pool,
            &uid.uid,
            &reference_addrs,
            chain_code,
            symbol,
            is_multisig,
            min_value,
            start,
            end,
            tx_kinds,
            page,
            page_size,
        )
        .await?;

        let selected_addrs = reference_addrs.into_iter().collect::<HashSet<String>>();
        let mut data = Vec::with_capacity(res.data.len());
        for item in &res.data {
            let from_hit = selected_addrs.contains(item.from_addr.as_str());
            let to_hit = selected_addrs.contains(item.to_addr.as_str());
            let transfer_type = Self::resolve_transfer_type(
                item.trade_type,
                &item.from_addr,
                &item.to_addr,
                &selected_addrs,
            );
            tracing::debug!(
                from_addr = %Self::mask_addr(&item.from_addr),
                to_addr = %Self::mask_addr(&item.to_addr),
                from_hit,
                to_hit,
                transfer_type,
                trade_type = item.trade_type as u8,
                "api_bill_lists transfer direction resolved"
            );
            data.push(self.convert_to_bill_entity(item, transfer_type));
        }

        let bill_res: Pagination<BillEntity> = Pagination::<BillEntity> {
            page: res.page,
            page_size: res.page_size,
            total_count: res.total_count,
            data,
        };
        Ok(bill_res)
    }

    pub async fn recent_bill(
        &self,
        token: &str,
        addr: &str,
        chain_code: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<RecentBillListVo>, ServiceError> {
        let api_transaction_pool = self.ctx.api_transaction_pool()?;
        let res = ApiWithdrawRepo::recent_bill(
            &api_transaction_pool,
            token,
            addr,
            chain_code,
            page,
            page_size,
        )
        .await;
        let mut data: Vec<RecentBillListVo> = vec![];
        let mut total_count = 0;
        match res {
            Ok(res) => {
                for it in res.data {
                    let transfer_type =
                        if it.trade_type == ApiTradeType::SelfRecharge { 0 } else { 1 };
                    let transaction_time = it.transaction_time.unwrap_or_else(Utc::now);
                    data.push(RecentBillListVo {
                        chain_code: it.chain_code,
                        symbol: it.symbol,
                        tx_hash: it.tx_hash.unwrap_or_default(),
                        value: it.value,
                        address: it.to_addr,
                        transaction_time,
                        transfer_type,
                        created_at: it.created_at,
                    })
                }
                total_count = res.total_count;
            }
            Err(_) => {}
        }

        let bill_res: Pagination<RecentBillListVo> =
            Pagination::<RecentBillListVo> { page, page_size, total_count, data };
        Ok(bill_res)
    }

    pub async fn query_tx_result(&self, req: Vec<String>) -> Result<Vec<BillEntity>, ServiceError> {
        let api_transaction_pool = self.ctx.api_transaction_pool()?;
        let core_pool = self.ctx.core_pool()?;
        let mut res = vec![];
        for id in req.iter() {
            match self.sync_bill_info(core_pool.clone(), &api_transaction_pool, id).await {
                Ok(tx) => res.push(tx),
                Err(e) => {
                    tracing::warn!("sync bill err id = {},err = {}", id, e)
                }
            }
        }
        Ok(res)
    }

    async fn sync_bill_info(
        &self,
        core_pool: CoreDbPool,
        api_transaction_pool: &ApiTransactionDbPool,
        id: &str,
    ) -> Result<BillEntity, ServiceError> {
        let bill = ApiWithdrawRepo::get_api_withdraw_by_id(api_transaction_pool, id).await?;

        if !Self::should_query_chain_result(bill.status) {
            let transfer_type = Self::default_transfer_type_by_trade_type(bill.trade_type);
            let e = self.convert_to_bill_entity(&bill, transfer_type);
            return Ok(e);
        }

        let sync_bill = match self.get_tx_res(&bill).await? {
            Some(tx_result) => tx_result,
            None => {
                // // 处理交易是否失败的逻辑
                // if bill.is_failed() {
                //     BillRepo::update_fail(&transaction.hash, &pool).await?;
                // }
                let transfer_type = Self::default_transfer_type_by_trade_type(bill.trade_type);
                let e = self.convert_to_bill_entity(&bill, transfer_type);
                return Ok(e);
            }
        };

        match self.handle_pending_tx_status(&sync_bill, core_pool).await? {
            Some(tx) => Ok(tx),
            None => {
                let transfer_type = Self::default_transfer_type_by_trade_type(bill.trade_type);
                let e = self.convert_to_bill_entity(&bill, transfer_type);
                Ok(e)
            }
        }
    }

    async fn handle_pending_tx_status(
        &self,
        sync_bill: &SyncBillEntity,
        pool: CoreDbPool,
    ) -> Result<Option<BillEntity>, ServiceError> {
        // 1. 更新账单
        let tx_result = BillRepo::update(&sync_bill.tx_update, pool.as_ref()).await?;

        // 2. 更新余额
        // ApiAssetsRepo::update_balance(
        //     pool,
        //     &transaction.owner,
        //     &transaction.chain_code,
        //     transaction.token.clone(),
        //     &sync_bill.balance,
        // )
        // .await?;

        Ok(tx_result)
    }

    async fn get_tx_res(
        &self,
        transaction: &ApiWithdrawEntity,
    ) -> Result<Option<SyncBillEntity>, ServiceError> {
        let adapter = ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
            self.ctx,
            &transaction.chain_code,
        )
        .await?;

        let tx_hash = match transaction.tx_hash {
            Some(ref tx_hash) if !tx_hash.is_empty() => tx_hash,
            _ => return Ok(None),
        };

        let Some(tx_result) = adapter.query_tx_res(tx_hash).await? else {
            return Ok(None);
        };

        let token_key = transaction.token_addr.clone();

        // 查询余额
        let balance = adapter.balance_token_key(&transaction.from_addr, token_key.clone()).await?;

        let coin = ApiCoinDomain::get_coin_by_token_key_exact_with_ctx(
            self.ctx,
            &transaction.chain_code,
            token_key,
        )
        .await?;

        let balance = unit::format_to_string(balance, coin.decimals)?;

        let tx_bill = BillRepo::build_bill_update(
            tx_result.hash,
            tx_result.transaction_fee.to_string(),
            tx_result.transaction_time,
            tx_result.status,
            tx_result.block_height,
            tx_result.resource_consume,
        );

        let sync_bill = SyncBillEntity { tx_update: tx_bill, balance };

        Ok(Some(sync_bill))
    }

    fn resolve_transfer_type(
        trade_type: ApiTradeType,
        from_addr: &str,
        to_addr: &str,
        selected_addrs: &HashSet<String>,
    ) -> i8 {
        let from_hit = selected_addrs.contains(from_addr);
        let to_hit = selected_addrs.contains(to_addr);
        if from_hit && !to_hit {
            1
        } else if to_hit && !from_hit {
            0
        } else {
            Self::default_transfer_type_by_trade_type(trade_type)
        }
    }

    fn default_transfer_type_by_trade_type(trade_type: ApiTradeType) -> i8 {
        if trade_type == ApiTradeType::SelfRecharge { 0 } else { 1 }
    }

    fn operation_resource_consume(
        operation: &ApiResourceOperationEntity,
    ) -> Result<Option<ApiWalletResourceConsume>, ServiceError> {
        let Some(raw_tx) = operation.raw_tx.as_deref().filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let raw_tx = wallet_utils::serde_func::serde_from_str::<RawTx>(raw_tx)?;
        match raw_tx {
            RawTx::Tron(_, resource_consume, _) => {
                Ok(Some(ApiWalletResourceConsume::from_bill_resource_consume(resource_consume)))
            }
            _ => Ok(None),
        }
    }

    fn convert_resource_operation_to_bill_entity(
        operation: &ApiResourceOperationEntity,
    ) -> BillEntity {
        let transaction_time = operation
            .transaction_time
            .or(operation.last_broadcast_at)
            .unwrap_or(operation.created_at);
        let status = if operation.err_code.is_some() {
            3
        } else if operation.tx_status.as_deref() == Some("success")
            || operation.result_status.as_deref() == Some("success")
        {
            2
        } else {
            1
        };

        let tx_kind = match operation.operation_type {
            ApiResourceOperationType::Stake => match operation.resource_type {
                ApiResourceType::Bandwidth => BillKind::FreezeBandwidth,
                ApiResourceType::Energy => BillKind::FreezeEnergy,
            },
            ApiResourceOperationType::Unstake => match operation.resource_type {
                ApiResourceType::Bandwidth => BillKind::UnFreezeBandwidth,
                ApiResourceType::Energy => BillKind::UnFreezeEnergy,
            },
            ApiResourceOperationType::Vote => BillKind::Vote,
            ApiResourceOperationType::WithdrawReward => BillKind::WithdrawReward,
            ApiResourceOperationType::WithdrawUnfreeze => BillKind::WithdrawUnFreeze,
            ApiResourceOperationType::Delegate => match operation.resource_type {
                ApiResourceType::Bandwidth => BillKind::DelegateBandwidth,
                ApiResourceType::Energy => BillKind::DelegateEnergy,
            },
            ApiResourceOperationType::Undelegate => match operation.resource_type {
                ApiResourceType::Bandwidth => BillKind::UnDelegateBandwidth,
                ApiResourceType::Energy => BillKind::UnDelegateEnergy,
            },
            ApiResourceOperationType::BatchDelegate => match operation.resource_type {
                ApiResourceType::Bandwidth => BillKind::BatchDelegateBandwidth,
                ApiResourceType::Energy => BillKind::BatchDelegateEnergy,
            },
            ApiResourceOperationType::BatchUndelegate => match operation.resource_type {
                ApiResourceType::Bandwidth => BillKind::BatchUnDelegateBandwidth,
                ApiResourceType::Energy => BillKind::BatchUnDelegateEnergy,
            },
        };

        BillEntity {
            id: operation.id as i32,
            hash: operation.tx_hash.clone().unwrap_or_default(),
            chain_code: operation.chain_code.clone(),
            symbol: "TRX".to_string(),
            transfer_type: 1,
            tx_kind: tx_kind.to_i8(),
            owner: operation.owner_address.clone(),
            from_addr: operation.owner_address.clone(),
            to_addr: operation.receiver_address.clone().unwrap_or_default(),
            token: AssetTokenKey::Native,
            value: operation.amount.clone(),
            resource_consume: "".to_string(),
            transaction_fee: operation.transaction_fee.clone().unwrap_or_default(),
            transaction_time,
            status,
            is_multisig: 0,
            block_height: "".to_string(),
            queue_id: operation.resource_trade_no.clone(),
            notes: "".to_string(),
            signer: operation.owner_address.clone(),
            extra: operation.result_payload.clone().unwrap_or_default(),
            created_at: operation.created_at,
            updated_at: operation.updated_at,
        }
    }

    fn should_query_chain_result(status: ApiWithdrawStatus) -> bool {
        matches!(
            status,
            ApiWithdrawStatus::ConfirmSuccessReport
                | ApiWithdrawStatus::ConfirmFailureReport
                | ApiWithdrawStatus::SendingTxFailed
                | ApiWithdrawStatus::AuditReject
        )
    }

    async fn build_reference_addrs(
        &self,
        pool: &ApiWalletDbPool,
        addr: Option<String>,
        root_addr: Option<String>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Vec<String>, ServiceError> {
        if let Some(explicit_addr) = addr.filter(|item| !item.is_empty()) {
            return Ok(vec![explicit_addr]);
        }

        let chain_codes =
            if let Some(chain_code) = chain_code { vec![chain_code.to_string()] } else { vec![] };
        tracing::info!(
            root_addr = ?root_addr,
            account_id = ?account_id,
            chain_codes = ?chain_codes,
            chain_count = chain_codes.len(),
            "api_bill_lists.build_reference_addrs -> api_account_list"
        );
        let accounts: Vec<ApiAccountEntity> =
            ApiAccountRepo::api_account_list(pool, root_addr, account_id, chain_codes).await?;
        let fallback_addrs = accounts.into_iter().map(|item| item.address).collect::<Vec<_>>();
        Ok(Self::resolve_reference_addrs(None, fallback_addrs))
    }

    fn resolve_reference_addrs(addr: Option<String>, fallback_addrs: Vec<String>) -> Vec<String> {
        if let Some(explicit_addr) = addr.filter(|item| !item.is_empty()) {
            return vec![explicit_addr];
        }
        let mut seen = HashSet::new();
        fallback_addrs
            .into_iter()
            .filter(|item| !item.is_empty())
            .filter(|item| seen.insert(item.clone()))
            .collect()
    }

    fn mask_addr(addr: &str) -> String {
        if addr.len() <= 8 {
            addr.to_string()
        } else {
            format!("{}...{}", &addr[..4], &addr[addr.len() - 4..])
        }
    }

    fn convert_to_bill_entity(&self, bill: &ApiWithdrawEntity, transfer_type: i8) -> BillEntity {
        let tx_kind = if bill.trade_type == ApiTradeType::Withdraw {
            BillKind::ApiWithdraw
        } else {
            BillKind::Transfer
        };
        let transaction_time = bill.transaction_time.unwrap_or_else(Utc::now);
        let signed_evm_self_transfer = bill.trade_type == ApiTradeType::SelfWithdraw
            && crate::domain::api_wallet::trans::shared_mechanism::is_evm_chain(&bill.chain_code)
            && bill.raw_tx.as_deref().is_some_and(|raw| !raw.is_empty());
        // Receipt facts take precedence over legacy MQTT narrative status,
        // including rows overwritten before the account-change guard was added.
        let receipt_status = if signed_evm_self_transfer {
            if bill.chain_failed_at.is_some() {
                Some(3)
            } else if bill.chain_success_at.is_some() {
                Some(2)
            } else {
                match bill.status {
                    ApiWithdrawStatus::Success => Some(2),
                    ApiWithdrawStatus::Failure => Some(3),
                    _ => None,
                }
            }
        } else {
            None
        };
        let status = if let Some(status) = receipt_status {
            status
        } else if bill.status == ApiWithdrawStatus::ConfirmSuccessReport {
            2
        } else if bill.status == ApiWithdrawStatus::ConfirmFailureReport
            || bill.status == ApiWithdrawStatus::SendingTxFailed
            || bill.status == ApiWithdrawStatus::AuditReject
        {
            3
        } else {
            1
        };
        BillEntity {
            id: bill.id as i32,
            hash: bill.tx_hash.clone().unwrap_or_default(),
            chain_code: bill.chain_code.to_string(),
            symbol: bill.symbol.to_string(),
            transfer_type,
            tx_kind: tx_kind as i8,
            owner: bill.from_addr.to_string(),
            from_addr: bill.from_addr.to_string(),
            to_addr: bill.to_addr.to_string(),
            token: bill.token_addr.clone(),
            value: bill.value.to_string(),
            resource_consume: bill.resource_consume.to_string(),
            transaction_fee: bill.transaction_fee.to_string(),
            transaction_time,
            status,
            is_multisig: 0,
            block_height: bill.block_height.clone().unwrap_or_default(),
            queue_id: "".to_string(),
            notes: bill.notes.clone().unwrap_or_default(),
            signer: bill.from_addr.to_string(),
            extra: "".to_string(),
            created_at: bill.created_at,
            updated_at: bill.updated_at,
        }
    }
}

#[cfg(test)]
mod transfer_token_tests {
    use super::ApiTransService;
    use crate::{
        application::api_wallet_transfer::ApiTransferApplication,
        request::api_wallet::{trans::ApiBaseTransferReq, transfer::ApiTransferExReq},
    };
    use wallet_database::entities::{
        api_resource_operation::{
            ApiResourceOperationEntity, ApiResourceOperationStatus, ApiResourceOperationTaskSource,
            ApiResourceOperationType,
        },
        api_resource_type::ApiResourceType,
        asset_token_key::AssetTokenKey,
        bill::BillKind,
    };

    fn make_transfer_ex_req(token_address: Option<&str>) -> ApiTransferExReq {
        ApiTransferExReq {
            base: ApiBaseTransferReq {
                from: "from".to_string(),
                to: "to".to_string(),
                value: "1".to_string(),
                chain_code: "tron".to_string(),
                symbol: "TRX".to_string(),
                token_address: AssetTokenKey::from_raw(token_address),
                decimals: 0,
                request_resource_id: None,
                spend_all: false,
                notes: None,
                metadata: None,
            },
            password: "pwd".to_string(),
            fee_setting: "".to_string(),
            signer: None,
        }
    }

    #[test]
    fn build_api_transfer_base_native_token_uses_none_for_chain() {
        let params = make_transfer_ex_req(Some(""));
        let base =
            ApiTransferApplication::build_api_transfer_base(&params, &AssetTokenKey::Native, 6);
        assert_eq!(base.token_address, AssetTokenKey::Native);
    }

    #[test]
    fn build_api_transfer_base_contract_token_preserved_for_chain() {
        let params = make_transfer_ex_req(Some("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"));
        let token = AssetTokenKey::from_raw(Some("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"));
        let base = ApiTransferApplication::build_api_transfer_base(&params, &token, 6);
        assert_eq!(base.token_address, token);
    }

    #[test]
    fn api_bill_detail_primary_lookup_falls_back_on_row_not_found() {
        let row_not_found = wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
            sqlx::Error::RowNotFound,
        ));
        let repo_not_found = wallet_database::Error::NotFound("missing bill".to_string());
        let real_db_error = wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
            sqlx::Error::Protocol("connection closed".to_string()),
        ));

        assert!(ApiTransService::is_bill_detail_primary_lookup_not_found(&row_not_found));
        assert!(ApiTransService::is_bill_detail_primary_lookup_not_found(&repo_not_found));
        assert!(!ApiTransService::is_bill_detail_primary_lookup_not_found(&real_db_error));
    }

    #[test]
    fn api_resource_vote_operation_maps_to_vote_bill_detail_entity() {
        let now = chrono::Utc::now();
        let operation = ApiResourceOperationEntity {
            id: 7,
            uid: "uid_1".to_string(),
            task_source: ApiResourceOperationTaskSource::Client,
            operation_type: ApiResourceOperationType::Vote,
            resource_trade_no: "vote_trade_1".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "TWithdrawOwner".to_string(),
            receiver_address: None,
            resource_type: ApiResourceType::Bandwidth,
            amount: "1010".to_string(),
            status: ApiResourceOperationStatus::Pending,
            task_ack_sent_at: None,
            building_at: None,
            raw_tx: None,
            tx_hash: Some("vote_hash".to_string()),
            transaction_fee: Some("0".to_string()),
            last_broadcast_at: Some(now),
            transaction_time: None,
            tx_status: Some("success".to_string()),
            tx_exec_receipt_uploaded_at: None,
            result_status: Some("success".to_string()),
            result_received_at: None,
            result_ack_sent_at: None,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            recover_status: None,
            next_retry_at: None,
            retry_count: 0,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            created_at: now,
            updated_at: None,
        };

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.hash, "vote_hash");
        assert_eq!(bill.owner, "TWithdrawOwner");
        assert_eq!(bill.from_addr, "TWithdrawOwner");
        assert_eq!(bill.tx_kind, BillKind::Vote.to_i8());
        assert_eq!(bill.value, "1010");
        assert_eq!(bill.status, 2);
        assert_eq!(bill.queue_id, "vote_trade_1");
    }

    #[test]
    fn api_resource_stake_operation_maps_to_freeze_bill_detail_entity() {
        let now = chrono::Utc::now();
        let operation = test_resource_operation(
            ApiResourceOperationType::Stake,
            ApiResourceType::Bandwidth,
            "stake_hash",
            "stake_trade_1",
            "100",
            now,
        );

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.hash, "stake_hash");
        assert_eq!(bill.tx_kind, BillKind::FreezeBandwidth.to_i8());
        assert_eq!(bill.value, "100");
    }

    #[test]
    fn api_resource_stake_operation_stays_confirming_after_broadcast_only() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::Stake,
            ApiResourceType::Energy,
            "stake_hash",
            "stake_trade_1",
            "100",
            now,
        );
        operation.tx_status = None;
        operation.result_status = None;

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::FreezeEnergy.to_i8());
        assert_eq!(bill.hash, "stake_hash");
        assert_eq!(bill.status, 1);
    }

    #[test]
    fn api_resource_unstake_operation_stays_confirming_after_broadcast_only() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::Unstake,
            ApiResourceType::Bandwidth,
            "unstake_hash",
            "unstake_trade_1",
            "50",
            now,
        );
        operation.tx_status = None;
        operation.result_status = None;

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::UnFreezeBandwidth.to_i8());
        assert_eq!(bill.hash, "unstake_hash");
        assert_eq!(bill.status, 1);
    }

    #[test]
    fn api_resource_vote_operation_stays_confirming_after_broadcast_only() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::Vote,
            ApiResourceType::Bandwidth,
            "vote_hash",
            "vote_trade_1",
            "1010",
            now,
        );
        operation.tx_status = None;
        operation.result_status = None;

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::Vote.to_i8());
        assert_eq!(bill.hash, "vote_hash");
        assert_eq!(bill.status, 1);
    }

    #[test]
    fn api_resource_unstake_operation_maps_to_unfreeze_bill_detail_entity() {
        let now = chrono::Utc::now();
        let operation = test_resource_operation(
            ApiResourceOperationType::Unstake,
            ApiResourceType::Energy,
            "unstake_hash",
            "unstake_trade_1",
            "50",
            now,
        );

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.hash, "unstake_hash");
        assert_eq!(bill.tx_kind, BillKind::UnFreezeEnergy.to_i8());
        assert_eq!(bill.value, "50");
    }

    #[test]
    fn api_resource_operation_batch_delegate_maps_to_batch_delegate_detail() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::BatchDelegate,
            ApiResourceType::Energy,
            "delegate_hash",
            "delegate_trade_1",
            "2",
            now,
        );
        operation.receiver_address = Some("TReceiver".to_string());

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::BatchDelegateEnergy.to_i8());
        assert_eq!(bill.to_addr, "TReceiver");
        assert_eq!(bill.value, "2");
    }

    #[test]
    fn api_resource_operation_batch_undelegate_maps_to_batch_undelegate_detail() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::BatchUndelegate,
            ApiResourceType::Bandwidth,
            "undelegate_hash",
            "undelegate_trade_1",
            "3",
            now,
        );
        operation.receiver_address = Some("TReceiver".to_string());

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::BatchUnDelegateBandwidth.to_i8());
        assert_eq!(bill.to_addr, "TReceiver");
        assert_eq!(bill.value, "3");
    }

    #[test]
    fn api_resource_operation_delegate_maps_to_delegate_detail() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::Delegate,
            ApiResourceType::Bandwidth,
            "delegate_hash",
            "delegate_trade_1",
            "2",
            now,
        );
        operation.receiver_address = Some("TReceiver".to_string());

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::DelegateBandwidth.to_i8());
        assert_eq!(bill.to_addr, "TReceiver");
    }

    #[test]
    fn api_resource_operation_undelegate_maps_to_undelegate_detail() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::Undelegate,
            ApiResourceType::Energy,
            "undelegate_hash",
            "undelegate_trade_1",
            "3",
            now,
        );
        operation.receiver_address = Some("TReceiver".to_string());

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::UnDelegateEnergy.to_i8());
        assert_eq!(bill.to_addr, "TReceiver");
    }

    #[test]
    fn api_resource_withdraw_reward_operation_maps_to_reward_bill_detail_entity() {
        let now = chrono::Utc::now();
        let operation = ApiResourceOperationEntity {
            id: 8,
            uid: "uid_1".to_string(),
            task_source: ApiResourceOperationTaskSource::Client,
            operation_type: ApiResourceOperationType::WithdrawReward,
            resource_trade_no: "reward_trade_1".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "TWithdrawOwner".to_string(),
            receiver_address: None,
            resource_type: ApiResourceType::Bandwidth,
            amount: "12.5".to_string(),
            status: ApiResourceOperationStatus::Pending,
            task_ack_sent_at: None,
            building_at: None,
            raw_tx: None,
            tx_hash: Some("reward_hash".to_string()),
            transaction_fee: Some("0".to_string()),
            last_broadcast_at: Some(now),
            transaction_time: None,
            tx_status: Some("success".to_string()),
            tx_exec_receipt_uploaded_at: None,
            result_status: Some("success".to_string()),
            result_received_at: None,
            result_ack_sent_at: None,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            recover_status: None,
            next_retry_at: None,
            retry_count: 0,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            created_at: now,
            updated_at: None,
        };

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.hash, "reward_hash");
        assert_eq!(bill.owner, "TWithdrawOwner");
        assert_eq!(bill.tx_kind, BillKind::WithdrawReward.to_i8());
        assert_eq!(bill.value, "12.5");
        assert_eq!(bill.status, 2);
        assert_eq!(bill.queue_id, "reward_trade_1");
    }

    #[test]
    fn api_resource_withdraw_unfreeze_operation_maps_to_withdraw_unfreeze_bill_detail_entity() {
        let now = chrono::Utc::now();
        let operation = test_resource_operation(
            ApiResourceOperationType::WithdrawUnfreeze,
            ApiResourceType::Bandwidth,
            "withdraw_unfreeze_hash",
            "withdraw_unfreeze_trade_1",
            "10",
            now,
        );

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.hash, "withdraw_unfreeze_hash");
        assert_eq!(bill.owner, "TWithdrawOwner");
        assert_eq!(bill.tx_kind, BillKind::WithdrawUnFreeze.to_i8());
        assert_eq!(bill.value, "10");
        assert_eq!(bill.status, 2);
        assert_eq!(bill.queue_id, "withdraw_unfreeze_trade_1");
    }

    #[test]
    fn api_resource_withdraw_unfreeze_operation_stays_confirming_after_broadcast_only() {
        let now = chrono::Utc::now();
        let mut operation = test_resource_operation(
            ApiResourceOperationType::WithdrawUnfreeze,
            ApiResourceType::Bandwidth,
            "withdraw_unfreeze_hash",
            "withdraw_unfreeze_trade_1",
            "10",
            now,
        );
        operation.tx_status = None;
        operation.result_status = None;

        let bill = ApiTransService::convert_resource_operation_to_bill_entity(&operation);

        assert_eq!(bill.tx_kind, BillKind::WithdrawUnFreeze.to_i8());
        assert_eq!(bill.hash, "withdraw_unfreeze_hash");
        assert_eq!(bill.status, 1);
    }

    fn test_resource_operation(
        operation_type: ApiResourceOperationType,
        resource_type: ApiResourceType,
        hash: &str,
        trade_no: &str,
        amount: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> ApiResourceOperationEntity {
        ApiResourceOperationEntity {
            id: 1,
            uid: "uid_1".to_string(),
            task_source: ApiResourceOperationTaskSource::Client,
            operation_type,
            resource_trade_no: trade_no.to_string(),
            chain_code: "tron".to_string(),
            owner_address: "TWithdrawOwner".to_string(),
            receiver_address: None,
            resource_type,
            amount: amount.to_string(),
            status: ApiResourceOperationStatus::Pending,
            task_ack_sent_at: None,
            building_at: None,
            raw_tx: None,
            tx_hash: Some(hash.to_string()),
            transaction_fee: Some("0".to_string()),
            last_broadcast_at: Some(now),
            transaction_time: None,
            tx_status: Some("success".to_string()),
            tx_exec_receipt_uploaded_at: None,
            result_status: Some("success".to_string()),
            result_received_at: None,
            result_ack_sent_at: None,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            recover_status: None,
            next_retry_at: None,
            retry_count: 0,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            created_at: now,
            updated_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected_addrs(addrs: &[&str]) -> HashSet<String> {
        addrs.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn resolve_transfer_type_returns_incoming_when_to_addr_matches_selected_account() {
        let selected = selected_addrs(&["to-address"]);
        let transfer_type = ApiTransService::resolve_transfer_type(
            ApiTradeType::SelfWithdraw,
            "from-address",
            "to-address",
            &selected,
        );
        assert_eq!(transfer_type, 0);
    }

    #[test]
    fn resolve_transfer_type_returns_outgoing_when_from_addr_matches_selected_account() {
        let selected = selected_addrs(&["from-address"]);
        let transfer_type = ApiTransService::resolve_transfer_type(
            ApiTradeType::SelfRecharge,
            "from-address",
            "to-address",
            &selected,
        );
        assert_eq!(transfer_type, 1);
    }

    #[test]
    fn resolve_transfer_type_falls_back_to_trade_type_when_both_addrs_match() {
        let selected = selected_addrs(&["from-address", "to-address"]);
        let transfer_type = ApiTransService::resolve_transfer_type(
            ApiTradeType::SelfRecharge,
            "from-address",
            "to-address",
            &selected,
        );
        assert_eq!(transfer_type, 0);
    }

    #[test]
    fn resolve_transfer_type_falls_back_to_trade_type_when_neither_addr_matches() {
        let selected = selected_addrs(&["another-address"]);
        let transfer_type = ApiTransService::resolve_transfer_type(
            ApiTradeType::SelfWithdraw,
            "from-address",
            "to-address",
            &selected,
        );
        assert_eq!(transfer_type, 1);
    }

    #[test]
    fn query_tx_result_status_gate_allows_only_sync_statuses() {
        let sync_statuses = [
            ApiWithdrawStatus::ConfirmSuccessReport,
            ApiWithdrawStatus::ConfirmFailureReport,
            ApiWithdrawStatus::SendingTxFailed,
            ApiWithdrawStatus::AuditReject,
        ];
        for status in sync_statuses {
            assert!(
                ApiTransService::should_query_chain_result(status),
                "{status:?} should enter chain-result sync"
            );
        }

        let local_statuses = [
            ApiWithdrawStatus::InitOrder,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::AuditPass,
            ApiWithdrawStatus::SendingTx,
            ApiWithdrawStatus::SendingTxReport,
            ApiWithdrawStatus::SendingTxFailedReport,
            ApiWithdrawStatus::Success,
            ApiWithdrawStatus::Failure,
        ];
        for status in local_statuses {
            assert!(
                !ApiTransService::should_query_chain_result(status),
                "{status:?} should return local bill data"
            );
        }
    }

    #[test]
    fn resolve_reference_addrs_prefers_explicit_addr_over_fallback() {
        let resolved = ApiTransService::resolve_reference_addrs(
            Some("THLja2cJJxjbn4cUZZq6BRX8QHK1sxFbT4".to_string()),
            vec![
                "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5".to_string(),
                "TNG39Z4DZZj1Qgb5TLbHiUfZG1QtdmBcxv".to_string(),
            ],
        );
        assert_eq!(resolved, vec!["THLja2cJJxjbn4cUZZq6BRX8QHK1sxFbT4".to_string()]);
    }
}
