use crate::{
    domain,
    domain::{
        bill::BillDomain,
        chain::{
            adapter::{ChainAdapterFactory, MultisigAdapter},
            transaction::ChainTransDomain,
        },
        coin::CoinDomain,
    },
    request::transaction::{self},
    response_vo::{
        self,
        standard_wallet::{
            account::Balance,
            transaction::{BillDetailVo, TransactionResult},
        },
    },
};
use wallet_database::{
    CoreDbPool, entities,
    entities::{
        asset_token_key::AssetTokenKey,
        assets::AssetsId,
        bill::{BillEntity, BillKind, BillStatus, RecentBillListVo, SyncBillEntity},
        multisig_account::{MultisigAccountPayStatus, MultisigAccountStatus},
        multisig_queue::{MemberSignedResult, MultisigQueueStatus},
    },
    pagination::Pagination,
    repositories::{
        account::AccountRepo, address_book::AddressBookRepo, assets::AssetsRepo, bill::BillRepo,
        coin::CoinRepo, multisig_account::MultisigAccountRepo, multisig_queue::MultisigQueueRepo,
    },
};
use wallet_types::constant::chain_code;
use wallet_utils::{RetryableError as _, unit};

pub struct TransactionService {
    ctx: &'static crate::context::Context,
}

impl TransactionService {
    pub fn new(ctx: &'static crate::context::Context) -> Self {
        Self { ctx }
    }

    // 本币的余额
    pub async fn chain_balance(
        &self,
        address: &str,
        chain_code: &str,
        symbol: &str,
        token_key: AssetTokenKey,
    ) -> Result<Balance, crate::error::service::ServiceError> {
        tracing::info!(
            address = %address,
            chain_code = %chain_code,
            request_symbol = %symbol,
            request_token_address = %token_key.as_db_str(),
            "chain_balance request start"
        );
        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, chain_code).await?;

        let coin = match CoinDomain::get_coin_by_token_key_with_ctx(
            self.ctx,
            chain_code,
            token_key.clone(),
        )
        .await
        {
            Ok(coin) => coin,
            Err(error) => {
                tracing::warn!(
                    chain_code = %chain_code,
                    request_symbol = %symbol,
                    request_token_address = %token_key.as_db_str(),
                    error = %error,
                    "chain_balance failed to resolve coin metadata"
                );
                return Err(error);
            }
        };
        tracing::info!(
            address = %address,
            chain_code = %chain_code,
            request_symbol = %symbol,
            resolved_symbol = %coin.symbol,
            request_token_address = %token_key.as_db_str(),
            resolved_token_address = ?coin.token_address.as_db_str(),
            "chain_balance resolved coin metadata"
        );
        let resolved_token_key = coin.token_address.clone();
        let resolved_token_address = resolved_token_key.to_option_string_for_api();
        let ticket = self
            .ctx
            .ordinary_balance_order
            .begin(&AssetsId::new(address, chain_code, resolved_token_key.clone()))
            .await;
        let balance = match adapter.balance(address, resolved_token_key).await {
            Ok(balance) => balance,
            Err(error) => {
                tracing::warn!(
                    address = %address,
                    chain_code = %chain_code,
                    request_symbol = %symbol,
                    resolved_symbol = %coin.symbol,
                    request_token_address = %token_key.as_db_str(),
                    resolved_token_address = ?resolved_token_address,
                    error = %error,
                    "chain_balance failed to fetch on-chain balance"
                );
                return Err(error.into());
            }
        };
        tracing::info!(
            address = %address,
            chain_code = %chain_code,
            request_symbol = %symbol,
            resolved_symbol = %coin.symbol,
            request_token_address = %token_key.as_db_str(),
            resolved_token_address = ?resolved_token_address,
            on_chain_balance = %balance,
            "chain_balance fetched on-chain balance"
        );
        let format_balance = unit::format_to_string(balance, coin.decimals)?;
        tracing::info!(
            address = %address,
            chain_code = %chain_code,
            request_symbol = %symbol,
            resolved_symbol = %coin.symbol,
            request_token_address = %token_key.as_db_str(),
            resolved_token_address = ?resolved_token_address,
            formatted_balance = %format_balance,
            "chain_balance formatted balance"
        );
        let balance = Balance {
            balance: format_balance.clone(),
            decimals: coin.decimals,
            original_balance: balance.to_string(),
        };

        ChainTransDomain::update_balance_ordered(
            self.ctx,
            address,
            chain_code,
            &coin.symbol,
            coin.token_address.clone(),
            &format_balance,
            &ticket,
        )
        .await?;

        tracing::info!(
            address = %address,
            chain_code = %chain_code,
            request_symbol = %symbol,
            resolved_symbol = %coin.symbol,
            request_token_address = %token_key.as_db_str(),
            resolved_token_address = ?resolved_token_address,
            coin_decimals = coin.decimals,
            "chain_balance request success"
        );

        Ok(balance)
    }

    /// 计算交易的手续费
    pub async fn transaction_fee(
        &self,
        mut params: transaction::BaseTransferReq,
    ) -> Result<response_vo::EstimateFeeResp, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        let token_key = params.token_address.clone();
        let coin = CoinRepo::coin_by_chain_token_key(&params.chain_code, token_key, &pool).await?;

        params.with_decimals(coin.decimals);
        params.with_token(coin.token_address.clone());

        let main_coin = CoinRepo::main_coin(&params.chain_code, &pool).await?;

        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &params.chain_code)
                .await?;
        let backend_api = self.ctx.get_global_backend_api();
        let fee = adapter
            .estimate_fee_with_ctx(
                self.ctx,
                params,
                main_coin.symbol.as_str(),
                backend_api.as_ref(),
            )
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), fee);
        Ok(fee_resp)
    }

    pub async fn transfer(
        &self,
        params: transaction::TransferReq,
        bill_kind: BillKind,
    ) -> Result<TransactionResult, crate::error::service::ServiceError> {
        use tracing::Instrument as _;
        let node =
            ChainAdapterFactory::get_chain_node_with_ctx(self.ctx, &params.base.chain_code).await?;
        let rpc_id = crate::domain::chain::adapter::evm_submission::rpc_id(&node.rpc_url);
        let span =
            tracing::info_span!("ordinary_transfer",chain=%params.base.chain_code,rpc_id=%rpc_id);
        async move {
            let chain =
                wallet_types::chain::chain::ChainCode::try_from(params.base.chain_code.as_str())?;
            let evm = crate::domain::chain::is_evm_chain(chain);
            let headers = if crate::domain::chain::rpc_need_header(&node.rpc_url)? {
                Some(self.ctx.get_rpc_header().await?)
            } else {
                None
            };
            let adapter = crate::domain::chain::adapter::TransactionAdapter::new(
                chain,
                &node.rpc_url,
                headers.clone(),
                crate::domain::chain::ChainDomain::network_kind_from_node_network(&node.network),
            )?;
            let probe_client = if evm {
                wallet_transport::client::RpcClient::new(
                    &node.rpc_url,
                    headers,
                    Some(std::time::Duration::from_secs(2)),
                )
                .ok()
            } else {
                None
            };
            let private_key = ChainTransDomain::get_key(
                self.ctx,
                &params.base.from,
                &params.base.chain_code,
                &params.password,
                &params.signer,
            )
            .await?;

            let tx_hash =
                ChainTransDomain::transfer(self.ctx, params, bill_kind, &adapter, private_key)
                    .await?;
            if let Some(client) = probe_client {
                crate::domain::chain::adapter::evm_submission::observe(client, tx_hash.clone());
            }
            Ok(TransactionResult { tx_hash })
        }
        .instrument(span)
        .await
    }

    async fn handle_queue_member(
        bill: &BillEntity,
        pool: CoreDbPool,
    ) -> Option<Vec<MemberSignedResult>> {
        if !bill.signer.is_empty() {
            let signer = bill.signer.split(",").map(|s| s.to_string()).collect::<Vec<String>>();

            let mut result = vec![];
            for address in signer.iter() {
                let book = AddressBookRepo::find_by_address_chain(&pool, address, &bill.chain_code)
                    .await
                    .ok()
                    .flatten();
                let name = if let Some(book) = book { book.name } else { String::new() };

                let member = MemberSignedResult::new(&name, address, 0, 1);
                result.push(member);
            }
            return Some(result);
        }

        if bill.transfer_type != 1 || bill.queue_id.is_empty() {
            return None;
        }

        // 获取队列信息
        let queue = match domain::multisig::MultisigDomain::queue_by_id(
            &bill.queue_id,
            &pool.clone().into_inner(),
        )
        .await
        {
            Ok(queue) => queue,
            Err(_) => return None,
        };

        (MultisigQueueRepo::signed_result(
            &queue.id,
            &queue.account_id,
            &queue.permission_id,
            pool.clone(),
        )
        .await)
            .ok()
    }

    pub async fn bill_detail(
        &self,
        tx_hash: &str,
        owner: &str,
    ) -> Result<BillDetailVo, crate::error::service::ServiceError> {
        let tx_hash = BillDomain::handle_hash(tx_hash);

        let pool = self.ctx.core_pool()?;
        let core_pool = self.ctx.core_pool()?;

        let mut bill = BillRepo::get_by_hash_and_owner(&tx_hash, owner, &pool).await?;
        bill.truncate_to_8_decimals();

        let signature = Self::handle_queue_member(&bill, pool.clone()).await;

        let main_coin = CoinRepo::main_coin(&bill.chain_code, &pool).await?;

        let mut res = BillDetailVo::new(bill, main_coin.symbol, signature)?;

        // 根据地址和链获取钱包名称
        if !res.bill.to_addr.is_empty() {
            let account = AccountRepo::account_with_wallet(
                &res.bill.to_addr,
                &res.bill.chain_code,
                core_pool.clone(),
            )
            .await;
            if let Ok(account) = account {
                res.wallet_name = account.wallet_name;
                res.account_name = account.name;
            }
        }

        Ok(res)
    }

    pub async fn recent_bill(
        &self,
        token: &str,
        addr: &str,
        chain_code: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<RecentBillListVo>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        Ok(BillRepo::recent_bill(token, addr, chain_code, page, page_size, pool).await?)
    }

    pub(crate) async fn run_ordinary_receipt_observer(
        &self,
        shutdown: tokio::sync::broadcast::Receiver<()>,
        period: std::time::Duration,
    ) {
        crate::infrastructure::runtime::time::receipt_observer::run_receipt_observer(
            shutdown,
            period,
            "OrdinaryReceiptObserver",
            0,
            |mut cursor| async move {
                if let Err(error) = self.observe_pending_ordinary_page(&mut cursor).await {
                    let (error_kind, rpc_code) = Self::query_error_kind(&error);
                    tracing::warn!(
                        metric = "ordinary_transaction_query",
                        stage = "background_scan",
                        error_kind,
                        rpc_code,
                        "ordinary receipt scan will retry"
                    );
                }
                cursor
            },
        )
        .await;
    }

    pub(crate) async fn observe_pending_ordinary_page(
        &self,
        cursor: &mut i32,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.observe_pending_ordinary_page_with_timeout(cursor, std::time::Duration::from_secs(5))
            .await
    }

    async fn observe_pending_ordinary_page_with_timeout(
        &self,
        cursor: &mut i32,
        timeout: std::time::Duration,
    ) -> Result<(), crate::error::service::ServiceError> {
        use futures::{StreamExt, stream};
        if self.ctx.is_recovery_paused() {
            return Ok(());
        }
        let pool = self.ctx.core_pool()?;
        // Keyset pagination over durable bills, without an age or retry cutoff.
        // No database transaction or sender gate is held during remote queries.
        let rows: Vec<BillEntity> = sqlx::query_as(
            "SELECT * FROM bill WHERE status = 1 AND is_multisig = 0 AND tx_kind = 1 AND id > ? ORDER BY id LIMIT 32"
        ).bind(*cursor).fetch_all(pool.read_ref()).await
            .map_err(|error| wallet_database::Error::Database(error.into()))?;
        if rows.len() < 32 {
            *cursor = 0;
        } else if let Some(last) = rows.last() {
            *cursor = last.id;
        }
        stream::iter(rows)
            .for_each_concurrent(4, |row| {
                let pool = pool.clone();
                async move {
                    if self.ctx.is_recovery_paused() {
                        return;
                    }
                    let evm = row
                        .chain_code
                        .as_str()
                        .try_into()
                        .map(crate::domain::chain::is_evm_chain)
                        .unwrap_or(false);
                    if !evm {
                        return;
                    }
                    match crate::infrastructure::runtime::time::with_timeout(
                        timeout,
                        self.sync_bill_info(&row.id.to_string(), pool),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(error)) => Self::log_query_error(&row, "background_receipt", &error),
                        Err(_) => tracing::warn!(
                            metric = "ordinary_transaction_query",
                            stage = "background_receipt",
                            bill_id = row.id,
                            error_kind = "timeout",
                            "ordinary receipt observation timed out; later rounds will retry"
                        ),
                    }
                }
            })
            .await;
        Ok(())
    }

    pub async fn query_tx_result(
        &self,
        req: Vec<String>,
    ) -> Result<Vec<BillEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        let mut res = vec![];
        let mut first_error = None;
        let mut failed_count = 0;
        for id in req.iter() {
            match self.sync_bill_info(id, pool.clone()).await {
                Ok(tx) => res.push(tx),
                Err(error) => {
                    failed_count += 1;
                    let (error_kind, rpc_code) = Self::query_error_kind(&error);
                    tracing::warn!(
                        metric = "ordinary_transaction_query",
                        stage = "batch_item",
                        bill_id = %id,
                        error_kind,
                        rpc_code,
                        "transaction status query failed"
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        // Keep independent successful updates durable, but never disguise a
        // failed/partial batch as a successful empty or incomplete response.
        if let Some(error) = first_error {
            tracing::warn!(
                metric = "ordinary_transaction_query",
                stage = "batch",
                requested_count = req.len(),
                completed_count = res.len(),
                failed_count,
                "transaction status batch query failed"
            );
            return Err(error);
        }
        Ok(res)
    }

    pub(crate) fn query_error_kind(
        error: &crate::error::service::ServiceError,
    ) -> (&'static str, Option<i64>) {
        use crate::error::service::ServiceError;
        use wallet_transport::TransportError;
        match error {
            ServiceError::ChainInteract(wallet_chain_interact::Error::TransportError(
                TransportError::NodeResponseError(node),
            ))
            | ServiceError::Transport(TransportError::NodeResponseError(node)) => {
                ("rpc", Some(node.code))
            }
            _ if error.is_network_error() => ("network", None),
            ServiceError::Database(_) => ("database", None),
            ServiceError::ChainInteract(_) => ("chain", None),
            _ => ("service", None),
        }
    }

    fn log_query_error(
        transaction: &BillEntity,
        stage: &'static str,
        error: &crate::error::service::ServiceError,
    ) {
        let (error_kind, rpc_code) = Self::query_error_kind(error);
        // Never log raw errors: an RPC error can contain a credential-bearing
        // URL or an arbitrary upstream response body.
        tracing::warn!(
            metric = "ordinary_transaction_query",
            stage,
            bill_id = transaction.id,
            chain = %transaction.chain_code,
            tx_hash = %transaction.hash,
            error_kind,
            rpc_code,
            "transaction status query failed"
        );
    }

    async fn sync_bill_info(
        &self,
        id: &str,
        pool: CoreDbPool,
    ) -> Result<BillEntity, crate::error::service::ServiceError> {
        let transaction = BillRepo::find_by_id(id, &pool).await?;

        if transaction.status != wallet_database::entities::bill::BillStatus::Pending.to_i8() {
            return Ok(transaction);
        }

        // 不处理swap 类型的交易
        if transaction.tx_kind == BillKind::Swap.to_i8() {
            return Ok(transaction);
        }

        let ticket = self
            .ctx
            .ordinary_balance_order
            .begin(&AssetsId::new(
                &transaction.owner,
                &transaction.chain_code,
                transaction.token.clone().into(),
            ))
            .await;
        let sync_bill = match self.get_tx_res(&transaction).await? {
            Some(tx_result) => tx_result,
            None => {
                // Missing EVM receipts remain unknown even after a timeout.
                // Other bill kinds/chains retain their existing expiry policy.
                let expired = transaction.is_failed();
                tracing::info!(
                    metric = "ordinary_transaction_query",
                    stage = "receipt_missing",
                    bill_id = transaction.id,
                    chain = %transaction.chain_code,
                    tx_hash = %transaction.hash,
                    expired,
                    "transaction receipt not found"
                );
                let ordinary_evm = transaction.is_multisig == 0
                    && transaction.tx_kind == 1
                    && transaction
                        .chain_code
                        .as_str()
                        .try_into()
                        .map(crate::domain::chain::is_evm_chain)
                        .unwrap_or(false);
                if expired && !ordinary_evm {
                    BillRepo::update_fail(&transaction.hash, &pool)
                        .await
                        .map_err(crate::error::service::ServiceError::from)
                        .inspect_err(|error| {
                            Self::log_query_error(&transaction, "expiry", error)
                        })?;
                    return Ok(BillRepo::find_by_id(id, &pool).await?);
                }
                return Ok(transaction);
            }
        };

        // 对于服务费订单和部署多签账号订单，需要修改对应的多签账号的状态
        if sync_bill.tx_update.status == entities::bill::BillStatus::Success.to_i8() {
            Self::handle_tx_kind(&transaction, &pool).await?;
        }

        // Acquire before the DB transaction: lock order is always asset -> DB.
        // A stale balance must not suppress durable bill confirmation facts.
        let current = ticket.current().await;
        // query transaction and handle result
        let tx = pool.as_ref().begin().await.map_err(|e| {
            crate::error::service::ServiceError::System(crate::error::system::SystemError::Service(
                e.to_string(),
            ))
        })?;

        match Self::handle_pending_tx_status(&transaction, &sync_bill, tx, current.is_some())
            .await?
        {
            Some(tx) => Ok(tx),
            None => Ok(transaction),
        }
    }

    async fn handle_pending_tx_status(
        transaction: &BillEntity,
        sync_bill: &SyncBillEntity,
        mut tx: sqlx::Transaction<'static, sqlx::Sqlite>,
        apply_balance: bool,
    ) -> Result<Option<BillEntity>, crate::error::service::ServiceError> {
        let assets_id = AssetsId {
            chain_code: transaction.chain_code.clone(),
            address: transaction.owner.clone(),
            token_address: transaction.token.clone().into(),
        };

        // 2. 更新账单
        let tx_result = BillRepo::update(&sync_bill.tx_update, tx.as_mut()).await?;

        // 1. 更新余额
        if apply_balance {
            AssetsRepo::update_balance_with_executor(&mut tx, &assets_id, &sync_bill.balance)
                .await
                .map_err(|e| {
                    crate::error::service::ServiceError::System(
                        crate::error::system::SystemError::Service(e.to_string()),
                    )
                })?;
        }

        // 3. 如果queue_id 存在表示是多签交易，需要同步多签队列里面的状态
        if !transaction.queue_id.is_empty() {
            let status = if sync_bill.tx_update.status == BillStatus::Success.to_i8() {
                MultisigQueueStatus::Success
            } else {
                MultisigQueueStatus::Fail
            };
            MultisigQueueRepo::update_status_with_executor(
                &transaction.queue_id,
                status,
                tx.as_mut(),
            )
            .await?;
        }

        tx.commit().await.map_err(|e| {
            crate::error::service::ServiceError::System(crate::error::system::SystemError::Service(
                e.to_string(),
            ))
        })?;
        Ok(tx_result)
    }

    // 对不同kind的交易做不同类型的处理
    async fn handle_tx_kind(
        bill_detail: &BillEntity,
        pool: &CoreDbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let tx_kind = BillKind::try_from(bill_detail.tx_kind).unwrap();
        match tx_kind {
            // deploy multisig account
            BillKind::DeployMultiSign => {
                let account =
                    MultisigAccountRepo::find_by_condition(&pool, "deploy_hash", &bill_detail.hash)
                        .await?;

                if let Some(account) = account {
                    let status = MultisigAccountStatus::OnChain.to_i8();
                    MultisigAccountRepo::update_status(&pool, &account.id, Some(status), None)
                        .await?;
                }
            }
            // transfer multisig service fee
            BillKind::ServiceCharge => {
                let account =
                    MultisigAccountRepo::find_by_condition(&pool, "fee_hash", &bill_detail.hash)
                        .await?;

                if let Some(account) = account {
                    let status = MultisigAccountPayStatus::Paid.to_i8();
                    MultisigAccountRepo::update_status(&pool, &account.id, None, Some(status))
                        .await?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn get_tx_res(
        &self,
        transaction: &BillEntity,
    ) -> Result<Option<SyncBillEntity>, crate::error::service::ServiceError> {
        let token_key = transaction.token.clone();
        let (tx_result, balance) = if transaction.is_multisig == 1
            && !transaction.queue_id.is_empty()
            && matches!(
                transaction.chain_code.as_str(),
                chain_code::POLYGON
                    | chain_code::AVALANCHE
                    | chain_code::ARBITRUM
                    | chain_code::BASE
                    | chain_code::SCROLL
            ) {
            let adapter = ChainAdapterFactory::get_multisig_adapter_with_ctx(
                &self.ctx,
                &transaction.chain_code,
            )
            .await
            .inspect_err(|error| Self::log_query_error(transaction, "adapter", error))?;
            let Some(tx_result) = adapter
                .query_tx_res(&transaction.hash)
                .await
                .map_err(crate::error::service::ServiceError::from)
                .inspect_err(|error| Self::log_query_error(transaction, "receipt", error))?
            else {
                return Ok(None);
            };
            let MultisigAdapter::Ethereum(chain) = adapter else {
                unreachable!("new EVM Safe chains use the Ethereum multisig adapter")
            };
            let balance = chain
                .balance(&transaction.owner, token_key.to_chain_token_option())
                .await
                .map_err(crate::error::service::ServiceError::from)
                .inspect_err(|error| Self::log_query_error(transaction, "balance", error))?;
            (tx_result, balance)
        } else {
            let adapter = ChainAdapterFactory::get_transaction_adapter_with_ctx(
                &self.ctx,
                &transaction.chain_code,
            )
            .await
            .inspect_err(|error| Self::log_query_error(transaction, "adapter", error))?;
            let Some(tx_result) = adapter
                .query_tx_res(&transaction.hash)
                .await
                .map_err(crate::error::service::ServiceError::from)
                .inspect_err(|error| Self::log_query_error(transaction, "receipt", error))?
            else {
                return Ok(None);
            };
            let balance = adapter
                .balance(&transaction.owner, token_key.clone())
                .await
                .map_err(crate::error::service::ServiceError::from)
                .inspect_err(|error| Self::log_query_error(transaction, "balance", error))?;
            (tx_result, balance)
        };

        let coin = CoinDomain::get_coin_by_token_key_with_ctx(
            self.ctx,
            &transaction.chain_code,
            token_key,
        )
        .await
        .inspect_err(|error| Self::log_query_error(transaction, "coin_metadata", error))?;

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

    pub async fn list_by_hashs(
        &self,
        owner: String,
        hashs: Vec<String>,
    ) -> Result<Vec<BillEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        Ok(BillRepo::lists_by_hashs(&owner, hashs, &pool).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::TransactionService;
    use wallet_database::{
        SqliteContext,
        entities::bill::{BillKind, BillStatus, SyncBillEntity},
        repositories::bill::BillRepo,
    };

    #[tokio::test]
    async fn pending_multisig_sync_rolls_back_when_queue_update_fails() {
        let dir = std::env::temp_dir().join(format!(
            "wallet_api_transaction_sync_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = SqliteContext::new(dir.to_str().unwrap(), Some("data.db")).await.unwrap();
        let pool = db.into_core_db_pool().unwrap();

        let mut new_bill = BillRepo::build_bill(
            "tx_sync_rollback".to_string(),
            "owner_sync_rollback".to_string(),
            "recipient_sync_rollback".to_string(),
            1.0,
            "ethereum".to_string(),
            "ETH".to_string(),
            true,
            BillKind::Transfer,
            String::new(),
        );
        new_bill.queue_id = "queue_sync_rollback".to_string();
        BillRepo::create(new_bill, &pool).await.unwrap();

        let bill =
            BillRepo::get_by_hash_and_owner("tx_sync_rollback", "owner_sync_rollback", &pool)
                .await
                .unwrap();
        sqlx::query("DROP TABLE multisig_queue").execute(pool.write_ref()).await.unwrap();

        let sync_bill = SyncBillEntity {
            tx_update: BillRepo::build_bill_update(
                "tx_sync_rollback".to_string(),
                "1".to_string(),
                1,
                BillStatus::Success.to_i8(),
                1,
                String::new(),
            ),
            balance: "10".to_string(),
        };
        let tx = pool.write_ref().begin().await.unwrap();

        let result =
            TransactionService::handle_pending_tx_status(&bill, &sync_bill, tx, true).await;

        assert!(result.is_err());
        let stored =
            BillRepo::get_by_hash_and_owner("tx_sync_rollback", "owner_sync_rollback", &pool)
                .await
                .unwrap();
        assert_eq!(stored.status, BillStatus::Pending.to_i8());
    }
}

#[cfg(test)]
#[path = "transaction_query_tests.rs"]
mod transaction_query_tests;
