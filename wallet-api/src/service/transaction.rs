use crate::domain;
use crate::domain::chain::adapter::ChainAdapterFactory;
use crate::request::transaction::{self, QueryBillReusltReq};
use crate::response_vo::{
    self,
    account::Balance,
    transaction::{BillDetailVo, TransactionResult},
};
use wallet_chain_interact::BillResourceConsume;
use wallet_database::dao::bill::BillDao;
use wallet_database::dao::multisig_account::MultisigAccountDaoV1;
use wallet_database::dao::multisig_queue::MultisigQueueDaoV1;
use wallet_database::entities;
use wallet_database::entities::assets::{AssetsEntity, AssetsId};
use wallet_database::entities::bill::{
    BillEntity, BillKind, BillStatus, BillUpdateEntity, RecentBillListVo, SyncBillEntity,
};
use wallet_database::entities::coin::CoinEntity;
use wallet_database::entities::multisig_account::{
    MultisigAccountPayStatus, MultisigAccountStatus,
};
use wallet_database::entities::multisig_queue::MultisigQueueStatus;
use wallet_database::pagination::Pagination;
use wallet_database::repositories::multisig_queue::MultisigQueueRepo;
use wallet_utils::unit;

pub struct TransactionService {}

impl TransactionService {
    // 本币的余额
    pub async fn chain_balance(
        address: &str,
        chain_code: &str,
        symbol: &str,
    ) -> Result<Balance, crate::ServiceError> {
        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(chain_code)
                .await?;

        let assets =
            domain::chain::transaction::ChainTransaction::assets(chain_code, symbol, address)
                .await?;

        let balance = adapter.balance(address, assets.token_address()).await?;
        let format_balance = unit::format_to_string(balance, assets.decimals)?;

        let balance = Balance {
            balance: format_balance.clone(),
            decimals: assets.decimals,
            original_balance: balance.to_string(),
        };

        domain::chain::transaction::ChainTransaction::update_balance(
            address,
            chain_code,
            symbol,
            &format_balance,
        )
        .await?;

        Ok(balance)
    }

    /// 计算交易的手续费
    pub async fn transaction_fee(
        mut params: transaction::BaseTransferReq,
    ) -> Result<response_vo::EstimateFeeResp, crate::ServiceError> {
        let assets = domain::chain::transaction::ChainTransaction::assets(
            &params.chain_code,
            &params.symbol,
            &params.from,
        )
        .await?;
        params.with_decimals(assets.decimals);
        params.with_token(assets.token_address());

        let main_coin =
            domain::chain::transaction::ChainTransaction::main_coin(&params.chain_code).await?;

        let adapter = ChainAdapterFactory::get_transaction_adapter(&params.chain_code).await?;
        let fee = adapter
            .estimate_fee(params, main_coin.symbol.as_str())
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), fee);
        Ok(fee_resp)
    }

    pub async fn transfer(
        params: transaction::TransferReq,
        bill_kind: BillKind,
    ) -> Result<TransactionResult, crate::ServiceError> {
        let adapter = ChainAdapterFactory::get_transaction_adapter(&params.base.chain_code).await?;

        let tx_hash =
            domain::chain::transaction::ChainTransaction::transfer(params, bill_kind, &adapter)
                .await?;
        Ok(TransactionResult { tx_hash })
    }

    pub async fn bill_detail(
        tx_hash: &str,
        transfer_type: i64,
    ) -> Result<BillDetailVo, crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;
        let mut bill = BillDao::get_by_hash_and_type(pool.as_ref(), tx_hash, transfer_type)
            .await?
            .ok_or(crate::BusinessError::Bill(crate::BillError::NotFound))?;
        bill.value = wallet_utils::unit::truncate_to_8_decimals(&bill.value);

        let sign = if bill.transfer_type == 1 && !bill.queue_id.is_empty() {
            let queue =
                domain::multisig::MultisigDomain::queue_by_id(&bill.queue_id, &pool).await?;

            // 如果是多签的转账,需要获取多签的信息
            let signature = MultisigQueueRepo::member_signed_result(
                &queue.account_id,
                &bill.queue_id,
                pool.clone(),
            )
            .await?;
            Some(signature)
        } else {
            None
        };

        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let main_coin = CoinEntity::main_coin(&bill.chain_code, pool.as_ref())
            .await?
            .ok_or(crate::BusinessError::Coin(crate::CoinError::NotFound(
                format!("chain = {}", bill.chain_code),
            )))?;

        let resource_consume = if !bill.resource_consume.is_empty() && bill.resource_consume != "0"
        {
            Some(BillResourceConsume::from_json_str(&bill.resource_consume)?)
        } else {
            None
        };

        let res = BillDetailVo {
            bill,
            resource_consume,
            signature: sign,
            fee_symbol: main_coin.symbol,
        };

        Ok(res)
    }

    pub async fn recent_bill(
        symbol: &str,
        addr: &str,
        chain_code: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<RecentBillListVo>, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        // let min_value = ConfigDomain::get_config_min_value(chain_code, symbol).await?;
        let min_value = None;

        let lists =
            BillDao::recent_bill(symbol, addr, chain_code, min_value, page, page_size, pool)
                .await?;
        Ok(lists)
    }

    pub async fn query_tx_result(
        req: Vec<QueryBillReusltReq>,
    ) -> Result<Vec<BillEntity>, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let mut res = vec![];
        for item in req.iter() {
            match Self::sync_bill_info(item, pool.clone()).await {
                Ok(tx) => res.push(tx),
                Err(e) => {
                    tracing::warn!("sync bill err tx_hash = {},err = {}", item.tx_hash, e)
                }
            }
        }
        Ok(res)
    }

    async fn sync_bill_info(
        query_rx: &QueryBillReusltReq,
        pool: wallet_database::DbPool,
    ) -> Result<BillEntity, crate::ServiceError> {
        let transaction =
            BillDao::get_by_hash_and_type(pool.as_ref(), &query_rx.tx_hash, query_rx.transfer_type)
                .await?
                .ok_or(crate::BusinessError::Bill(crate::BillError::NotFound))?;

        if transaction.status != wallet_database::entities::bill::BillStatus::Pending.to_i8() {
            return Ok(transaction);
        }

        let sync_bill = match Self::get_tx_res(&transaction).await? {
            Some(tx_result) => tx_result,
            None => {
                // 处理交易是否失败的逻辑
                if transaction.is_failed() {
                    BillDao::update_fail(&transaction.hash, pool.as_ref())
                        .await
                        .map_err(|e| {
                            crate::SystemError::Service(format!("update bill fail:{e:?}"))
                        })?;
                }
                return Ok(transaction);
            }
        };

        // 对于服务费订单和部署多签账号订单，需要修改对应的多签账号的状态
        if sync_bill.tx_update.status == entities::bill::BillStatus::Success.to_i8() {
            Self::handle_tx_kind(&transaction).await?;
        }

        // query transaction and handle result
        let tx = pool
            .begin()
            .await
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        match Self::handle_pending_tx_status(&transaction, &sync_bill, tx).await? {
            Some(tx) => Ok(tx),
            None => Ok(transaction),
        }
    }

    async fn handle_pending_tx_status(
        transaction: &BillEntity,
        sync_bill: &SyncBillEntity,
        mut tx: sqlx::Transaction<'static, sqlx::Sqlite>,
    ) -> Result<Option<BillEntity>, crate::ServiceError> {
        let assets_id = AssetsId {
            chain_code: transaction.chain_code.clone(),
            symbol: transaction.symbol.clone(),
            address: transaction.from_addr.clone(),
        };

        // 1. 更新余额
        AssetsEntity::update_balance(tx.as_mut(), &assets_id, &sync_bill.balance)
            .await
            .map_err(|e| crate::SystemError::Service(format!("update balance fail:{e:?}")))?;

        // 2. 更新账单
        let tx_result = BillDao::update(&sync_bill.tx_update, tx.as_mut())
            .await
            .map_err(|e| crate::SystemError::Service(format!("update bill fail:{e:?}")))?;

        // 3. 如果queue_id 存在表示是多签交易，需要同步多签队列里面的状态
        if !transaction.queue_id.is_empty() {
            let status = if sync_bill.tx_update.status == BillStatus::Success.to_i8() {
                MultisigQueueStatus::Success
            } else {
                MultisigQueueStatus::Fail
            };
            let _ =
                MultisigQueueDaoV1::update_status(&transaction.queue_id, status, tx.as_mut()).await;
        }

        let _res = tx.commit().await;
        Ok(tx_result)
    }

    // 对不同kind的交易做不同类型的处理
    async fn handle_tx_kind(bill_detail: &BillEntity) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let tx_kind = BillKind::try_from(bill_detail.tx_kind).unwrap();
        match tx_kind {
            // deploy multisig account
            BillKind::DeployMultiSign => {
                let condition = vec![("deploy_hash", bill_detail.hash.as_str())];
                let account = MultisigAccountDaoV1::find_by_conditions(condition, &*pool)
                    .await
                    .map_err(|e| crate::ServiceError::Database(e.into()))?;

                if let Some(account) = account {
                    let status = MultisigAccountStatus::OnChain.to_i8();
                    MultisigAccountDaoV1::update_status(&account.id, Some(status), None, &*pool)
                        .await
                        .map_err(crate::SystemError::Database)?;
                }
            }
            // transfer multisig service fee
            BillKind::ServiceCharge => {
                let condition = vec![("fee_hash", bill_detail.hash.as_str())];
                let account = MultisigAccountDaoV1::find_by_conditions(condition, &*pool)
                    .await
                    .map_err(|e| crate::ServiceError::Database(e.into()))?;

                if let Some(account) = account {
                    let status = MultisigAccountPayStatus::Paid.to_i8();
                    MultisigAccountDaoV1::update_status(&account.id, None, Some(status), &*pool)
                        .await
                        .map_err(crate::SystemError::Database)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn get_tx_res(
        transaction: &BillEntity,
    ) -> Result<Option<SyncBillEntity>, crate::ServiceError> {
        let adapter = domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(
            &transaction.chain_code,
        )
        .await?;

        let Some(tx_result) = adapter.query_tx_res(&transaction.hash).await? else {
            return Ok(None);
        };

        let token = transaction.token.as_ref().and_then(|token| {
            if token.is_empty() {
                None
            } else {
                Some(token.to_string())
            }
        });

        // 查询余额
        let balance = adapter.balance(&transaction.owner, token).await?;
        let assets = domain::chain::transaction::ChainTransaction::assets(
            &transaction.chain_code,
            &transaction.symbol,
            &transaction.owner,
        )
        .await?;

        let balance = unit::format_to_string(balance, assets.decimals)?;

        let tx_bill = BillUpdateEntity::new(
            tx_result.hash,
            tx_result.transaction_fee.to_string(),
            tx_result.transaction_time,
            tx_result.status,
            tx_result.block_height,
            tx_result.resource_consume,
        );

        let sync_bill = SyncBillEntity {
            tx_update: tx_bill,
            balance,
        };

        Ok(Some(sync_bill))
    }
}
