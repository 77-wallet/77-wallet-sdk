use crate::{
    domain::{
        self,
        chain::{
            TransferResp,
            adapter::{ChainAdapterFactory, MultisigAdapter},
            transaction::ChainTransDomain,
        },
        coin::CoinDomain,
        multisig::{MultisigDomain, MultisigQueueDomain},
        task_queue::TaskQueueDomain,
    },
    infrastructure::task_queue::{CommonTask, task::Tasks},
    request::transaction::Signer,
    response_vo::{
        self, MultisigQueueFeeParams,
        standard_wallet::{
            multisig_account::QueueInfo, multisig_transaction::MultisigQueueInfoVo,
            transaction::TransferParams,
        },
    },
};
use wallet_chain_interact::{
    BillResourceConsume, btc, eth, sol, sol::operations::SolInstructionOperation, tron,
    tron::operations::TronConstantOperation as _,
};
use wallet_database::{
    CoreDbPool, DbPool,
    entities::{
        bill::BillKind,
        multisig_queue::{
            MultisigQueueEntity, MultisigQueueStatus, NewMultisigQueueEntity, QueueTaskEntity,
            fail_reason,
        },
        multisig_signatures::{MultisigSignatureStatus, NewSignatureEntity},
    },
    pagination::Pagination,
    repositories::{
        bill::BillRepo, multisig_member::MultisigMemberRepo, multisig_queue::MultisigQueueRepo,
        permission::PermissionRepo,
    },
};
use wallet_transport_backend::{
    consts::endpoint,
    request::{PermissionData, SignedTranUpdateHashReq},
};
use wallet_types::constant::chain_code;
use wallet_utils::unit;

pub struct MultisigTransactionService {
    ctx: &'static crate::context::Context,
}

impl MultisigTransactionService {
    pub fn new(ctx: &'static crate::context::Context) -> Self {
        Self { ctx }
    }

    fn remap_tron_to_not_found_error(
        err: crate::error::service::ServiceError,
        req: &TransferParams,
    ) -> crate::error::service::ServiceError {
        if req.chain_code != chain_code::TRON {
            return err;
        }

        match err {
            crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(address),
                ),
            ) if address == req.to => {
                tracing::warn!(
                    msq_step = "remap_to_chain_error",
                    from = %req.from,
                    to = %req.to,
                    chain_code = %req.chain_code,
                    symbol = %req.symbol,
                    token = ?req.token_address,
                    "remap Account(NotFound(to)) to Chain(AddressNotInit) for tron multisig create"
                );
                crate::error::service::ServiceError::Business(
                    crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::AddressNotInit,
                    ),
                )
            }
            _ => err,
        }
    }

    pub async fn create_queue_fee(
        &self,
        req_params: MultisigQueueFeeParams,
    ) -> Result<response_vo::EstimateFeeResp, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;

        let account = MultisigDomain::account_by_address(&req_params.from, true, &pool).await?;

        let assets = ChainTransDomain::assets(
            self.ctx,
            &req_params.chain_code,
            &req_params.from,
            req_params.token_key(),
        )
        .await?;

        let main_coin = ChainTransDomain::main_coin(self.ctx, &assets.chain_code).await?;

        let adapter =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &account.chain_code)
                .await?;
        let asset_token = assets.token_key().to_chain_token_option();

        let res = adapter
            .build_multisig_fee(
                self.ctx,
                &req_params,
                &account,
                assets.decimals,
                asset_token,
                &main_coin.symbol,
            )
            .await?;
        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), res);

        Ok(fee_resp)
    }

    // Creates a new multisig transaction queue.
    pub async fn create_multisig_queue(
        &self,
        req: TransferParams,
        password: String,
    ) -> Result<String, crate::error::service::ServiceError> {
        if let Some(signer) = req.signer.clone() {
            self.create_with_permission(req, &password, signer).await
        } else {
            self.create_with_account(req, &password).await
        }
    }

    async fn create_with_account(
        &self,
        req: TransferParams,
        password: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        tracing::info!(
            msq_step = "create_with_account_start",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            symbol = %req.symbol,
            token = ?req.token_address,
            signer = ?req.signer,
            "start create multisig queue with account"
        );
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let assets =
            ChainTransDomain::assets(self.ctx, &req.chain_code, &req.from, req.token_key()).await?;
        let asset_token = assets.token_key().clone();
        tracing::info!(
            msq_step = "assets_loaded",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            symbol = %req.symbol,
            token = %asset_token.as_db_str(),
            "multisig create assets loaded"
        );

        let account = MultisigDomain::account_by_address(&req.from, true, &pool).await?;
        tracing::info!(
            msq_step = "multisig_account_loaded",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            account_id = %account.id,
            initiator = %account.initiator_addr,
            threshold = account.threshold,
            "multisig account loaded"
        );
        MultisigDomain::validate_queue(&account)?;
        tracing::info!(msq_step = "multisig_account_validated", account_id = %account.id);

        tracing::info!("[测试2391bug] 查询到多签账户信息：{:?}", account);
        let key = ChainTransDomain::get_key(
            self.ctx,
            &account.initiator_addr,
            &account.chain_code,
            password,
            &req.signer,
        )
        .await?;
        tracing::info!(
            msq_step = "initiator_key_loaded",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            "initiator key loaded for multisig create"
        );

        let adapter =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &account.chain_code)
                .await?;
        tracing::info!(
            msq_step = "build_multisig_with_account_start",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            symbol = %req.symbol,
            token = ?req.token_address,
            "begin building multisig transaction"
        );
        let rs = adapter
            .build_multisig_with_account(&req, &account, &assets, key)
            .await
            .map_err(|err| Self::remap_tron_to_not_found_error(err, &req))?;
        tracing::info!(
            msq_step = "build_multisig_with_account_done",
            tx_hash = %rs.tx_hash,
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            "built multisig transaction successfully"
        );

        let mut queue = NewMultisigQueueEntity::from(&req)
            .with_msg_hash(&rs.tx_hash)
            .with_raw_data(&rs.raw_data)
            .with_token(asset_token)
            .set_id();
        queue.account_id = account.id.clone();

        if queue.chain_code != chain_code::SOLANA {
            // 对多签队列进行签名
            MultisigQueueDomain::batch_sign_queue(
                self.ctx, &mut queue, password, &account, &adapter, &pool,
            )
            .await?;
        }

        queue.compute_status(account.threshold);

        // write multisig queue data to local database
        let res = MultisigQueueRepo::create_queue_with_sign(core_pool, &mut queue).await?;
        tracing::info!(
            msq_step = "queue_saved",
            queue_id = %res.id,
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            "multisig queue saved"
        );

        // 上报后端
        MultisigQueueDomain::upload_queue_backend(self.ctx, res.id, &pool, None, None).await?;
        tracing::info!(
            msq_step = "upload_queue_backend_done",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            "multisig queue uploaded"
        );

        Ok(rs.tx_hash)
    }

    async fn create_with_permission(
        &self,
        req: TransferParams,
        password: &str,
        signer: Signer,
    ) -> Result<String, crate::error::service::ServiceError> {
        tracing::info!(
            msq_step = "create_with_permission_start",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            symbol = %req.symbol,
            token = ?req.token_address,
            signer_address = %signer.address,
            signer_permission_id = signer.permission_id,
            "start create multisig queue with permission"
        );
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let coin =
            CoinDomain::get_coin_by_token_key_with_ctx(self.ctx, &req.chain_code, req.token_key())
                .await?;
        tracing::info!(
            msq_step = "permission_coin_loaded",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            symbol = %coin.symbol,
            token = %coin.token_address.as_db_str(),
            "permission flow coin loaded"
        );

        let permission = PermissionRepo::permission_with_user(
            &core_pool,
            &req.from,
            signer.permission_id,
            false,
        )
        .await?;

        let Some(p) = permission else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Permission(
                    crate::error::business::permission::PermissionError::ActivesPermissionNotFound,
                ),
            ));
        };

        let adapter =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &req.chain_code).await?;
        tracing::info!(
            msq_step = "build_multisig_with_permission_start",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            symbol = %req.symbol,
            token = ?req.token_address,
            signer_address = %signer.address,
            signer_permission_id = signer.permission_id,
            "begin building multisig transaction with permission"
        );
        let rs = adapter
            .build_multisig_with_permission(&req, &p.permission, &coin)
            .await
            .map_err(|err| Self::remap_tron_to_not_found_error(err, &req))?;
        tracing::info!(
            msq_step = "build_multisig_with_permission_done",
            tx_hash = %rs.tx_hash,
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            "built multisig transaction with permission"
        );

        let mut queue = NewMultisigQueueEntity::from(&req)
            .with_msg_hash(&rs.tx_hash)
            .with_raw_data(&rs.raw_data)
            .with_token(coin.token_address.clone())
            .set_id();
        queue.permission_id = p.permission.id.clone();

        if queue.chain_code != chain_code::SOLANA {
            // 对多签队列进行签名
            MultisigQueueDomain::batch_sign_with_permission(
                self.ctx, &mut queue, password, &p, &pool,
            )
            .await?;
        }

        queue.compute_status(p.permission.threshold as i32);

        // write multisig queue data to local database
        let res = MultisigQueueRepo::create_queue_with_sign(core_pool, &mut queue).await?;
        tracing::info!(
            msq_step = "queue_saved",
            queue_id = %res.id,
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            "permission multisig queue saved"
        );

        let opt = PermissionData { opt_address: signer.address.clone(), users: p.users() };

        // 上报后端
        MultisigQueueDomain::upload_queue_backend(self.ctx, res.id, &pool, None, Some(opt)).await?;
        tracing::info!(
            msq_step = "upload_queue_backend_done",
            from = %req.from,
            to = %req.to,
            chain_code = %req.chain_code,
            signer_address = %signer.address,
            "permission multisig queue uploaded"
        );

        Ok(rs.tx_hash)
    }

    pub async fn multisig_queue_list(
        &self,
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigQueueInfoVo>, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        // 先处理过期的交易
        let _ = MultisigQueueRepo::update_expired_queue(&core_pool).await;

        let mut lists = MultisigQueueRepo::queue_list(
            from,
            chain_code,
            status,
            page,
            page_size,
            core_pool.clone(),
        )
        .await?;

        let mut task = Tasks::new();
        let mut data = vec![];
        // every queue signed num
        for item in lists.data.iter_mut() {
            // if queue status has exec , add to task query result
            if item.status == MultisigQueueStatus::InConfirmation.to_i8() {
                task = task.push(CommonTask::QueryQueueResult(QueueTaskEntity {
                    id: item.id.clone(),
                    status: item.status,
                }));
            }

            let signature = MultisigQueueRepo::signed_result(
                &item.id,
                &item.account_id,
                &item.permission_id,
                core_pool.clone(),
            )
            .await?;

            let sign_num: i64 = signature
                .iter()
                .filter_map(|sig| if sig.singed != 0 { Some(1) } else { None })
                .sum();
            let extra = MultisigQueueDomain::handle_queue_extra(item, &pool).await?;

            data.push(MultisigQueueInfoVo {
                queue: item.clone(),
                extra: extra.unwrap_or_default(),
                signature,
                sign_num,
            });
        }

        task.send_with_ctx(self.ctx).await?;

        let result = Pagination {
            page: lists.page,
            page_size: lists.page_size,
            total_count: lists.total_count,
            data,
        };

        Ok(result)
    }

    // queue info
    pub async fn multisig_queue_info(
        &self,
        queue_id: &str,
    ) -> Result<MultisigQueueInfoVo, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let queue = MultisigQueueRepo::find_by_id_with_extra(queue_id, &core_pool).await?.ok_or(
            crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigQueue(
                    crate::error::business::multisig_queue::MultisigQueueError::NotFound,
                ),
            ),
        )?;

        let signature = MultisigQueueRepo::signed_result(
            &queue.id,
            &queue.account_id,
            &queue.permission_id,
            core_pool,
        )
        .await?;

        let sign_num: i64 =
            signature.iter().filter_map(|sig| if sig.singed == 1 { Some(1) } else { None }).sum();

        let extra = MultisigQueueDomain::handle_queue_extra(&queue, &pool).await?;

        Ok(MultisigQueueInfoVo { queue, signature, sign_num, extra: extra.unwrap_or_default() })
    }

    // only solana used
    pub async fn sign_fee(
        &self,
        queue_id: String,
        address: String,
    ) -> Result<response_vo::EstimateFeeResp, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let queue = MultisigDomain::queue_by_id(&queue_id, &pool).await?;
        let multisig_account =
            MultisigDomain::account_by_address(&queue.from_addr, true, &pool).await?;

        let adapter =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &queue.chain_code)
                .await?;

        let main_coin = ChainTransDomain::main_coin(self.ctx, &queue.chain_code).await?;

        let res = adapter
            .sign_fee(self.ctx, &multisig_account, &address, &queue.raw_data, &main_coin.symbol)
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), res);
        Ok(fee_resp)
    }

    // multisig sign message
    pub async fn sign_multisig_transaction(
        &self,
        queue_id: &str,
        status: i32,
        password: &str,
        address: Option<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let status = MultisigSignatureStatus::try_from(status)
            .map_err(|e| crate::error::service::ServiceError::Parameter(e.to_string()))?;

        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let queue = MultisigDomain::queue_by_id(queue_id, &pool).await?;
        MultisigQueueDomain::validate_queue(&queue, false)?;

        let sign_addr = Self::get_address_to_sign(pool.clone(), &queue, address).await?;
        // 1.签名
        let signed =
            self._sign_transaction(&queue, status, pool.clone(), password, sign_addr).await?;

        // 2.同步签名的结果状态
        MultisigQueueRepo::sync_sign_status(&queue, queue.status, core_pool.clone()).await?;

        // 3. 签名的结果发送给后端
        MultisigQueueDomain::upload_queue_sign(self.ctx, queue_id, pool, signed, status).await
    }

    // 查找需要进行签名的地址
    pub async fn get_address_to_sign(
        pool: DbPool,
        queue: &MultisigQueueEntity,
        address: Option<String>,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool);
        match address {
            Some(address) => Ok(vec![address]),
            None => {
                // 区分是多签还是普通权限
                if !queue.account_id.is_empty() {
                    let member =
                        MultisigQueueRepo::self_member_by_account(&queue.account_id, &core_pool)
                            .await?;

                    Ok(member.get_owner_str_vec())
                } else {
                    let users = PermissionRepo::self_user(&core_pool, &queue.permission_id).await?;

                    Ok(users.iter().map(|u| u.address.clone()).collect())
                }
            }
        }
    }

    pub async fn multisig_transfer_fee(
        &self,
        queue_id: &str,
    ) -> Result<response_vo::EstimateFeeResp, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let queue = MultisigDomain::queue_by_id(queue_id, &pool).await?;

        let coin = CoinDomain::get_coin_by_token_key_with_ctx(
            self.ctx,
            &queue.chain_code,
            queue.token_key(),
        )
        .await?;

        // 签名数
        let signs = MultisigQueueRepo::get_signed_list(&core_pool, queue_id).await?;
        let sign_list = signs.get_order_sign_str();

        let instance =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &queue.chain_code)
                .await?;
        let main_coin = ChainTransDomain::main_coin(self.ctx, &queue.chain_code).await?;

        let backend = self.ctx.get_global_sqlite_pool()?;
        let backend_api = self.ctx.get_global_backend_api();
        let fee = instance
            .estimate_fee_with_ctx(
                self.ctx,
                &queue,
                &coin,
                backend_api.as_ref(),
                sign_list,
                main_coin.symbol.as_str(),
                &backend,
            )
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), fee);
        Ok(fee_resp)
    }

    // 执行签名
    pub async fn _sign_transaction(
        &self,
        queue: &MultisigQueueEntity,
        status: MultisigSignatureStatus,
        pool: DbPool,
        password: &str,
        sign_addr: Vec<String>,
    ) -> Result<Vec<NewSignatureEntity>, crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        match status {
            MultisigSignatureStatus::Rejected | MultisigSignatureStatus::UnSigned => {
                let mut result = vec![];
                for address in sign_addr {
                    let params =
                        MultisigQueueRepo::build_signature(&queue.id, &address, "", status, None);

                    MultisigQueueRepo::create_or_update_sign(&params, &core_pool).await?;

                    result.push(params);
                }
                Ok(result)
            }
            MultisigSignatureStatus::Approved => {
                if !queue.account_id.is_empty() {
                    self.signed_account(queue, pool, sign_addr, password, status).await
                } else {
                    self.signed_permission(queue, pool, sign_addr, password, status).await
                }
            }
        }
    }

    async fn signed_account(
        &self,
        queue: &MultisigQueueEntity,
        pool: DbPool,
        sign_addr: Vec<String>,
        password: &str,
        status: MultisigSignatureStatus,
    ) -> Result<Vec<NewSignatureEntity>, crate::error::service::ServiceError> {
        let mut result = vec![];
        let core_pool = CoreDbPool::new(pool.clone());

        let multisig_account =
            MultisigDomain::account_by_address(&queue.from_addr, true, &pool).await?;
        // 当前已签名的数量
        let sign_list = MultisigQueueRepo::get_signed_list(&core_pool, &queue.id).await?;

        let need_sign = sign_list.need_signed_num(multisig_account.threshold as usize);
        if need_sign == 0 {
            return Ok(result);
        }

        let mut signed_num = 0;

        // 批量执行签名
        let instance =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &queue.chain_code)
                .await?;
        for i in 0..sign_addr.len() {
            let address = sign_addr.get(i).unwrap();
            // filter already signed
            if sign_list.contains_address(address) {
                continue;
            };

            let key =
                ChainTransDomain::get_key(self.ctx, address, &queue.chain_code, password, &None)
                    .await?;

            let rs =
                instance.sign_multisig_tx(&multisig_account, address, key, &queue.raw_data).await?;
            let params =
                MultisigQueueRepo::build_signature(&queue.id, address, &rs.signature, status, None);

            MultisigQueueRepo::create_or_update_sign(&params, &core_pool).await?;
            result.push(params);

            if queue.chain_code == chain_code::SOLANA {
                let tx = BillRepo::build_signed_bill(
                    rs.tx_hash,
                    address.clone(),
                    queue.chain_code.clone(),
                );
                let pool = self.ctx.core_pool()?;
                domain::bill::BillDomain::create_bill(&pool, tx).await?;
            }

            signed_num += 1;
            if signed_num >= need_sign {
                break;
            }
        }

        Ok(result)
    }

    // 目前只有tron链实现了权限相关的内容
    async fn signed_permission(
        &self,
        queue: &MultisigQueueEntity,
        pool: DbPool,
        sign_addr: Vec<String>,
        password: &str,
        status: MultisigSignatureStatus,
    ) -> Result<Vec<NewSignatureEntity>, crate::error::service::ServiceError> {
        let mut result = vec![];
        let core_pool = CoreDbPool::new(pool.clone());

        let permission = PermissionRepo::find_by_id(&core_pool, &queue.permission_id).await?;

        // 当前已签名的数量
        let sign_list = MultisigQueueRepo::signed_result(
            &queue.id,
            &queue.account_id,
            &queue.permission_id,
            core_pool.clone(),
        )
        .await?;

        // 需要签名的阈值
        let total_weight =
            sign_list.iter().filter(|s| !s.signature.is_empty()).map(|s| s.weight).sum::<i64>();

        let need_sign = (permission.threshold - total_weight).max(0);
        if need_sign == 0 {
            return Ok(result);
        }

        // 批量执行签名
        for i in 0..sign_addr.len() {
            let address = sign_addr.get(i).unwrap();
            // filter already signed
            if sign_list.iter().any(|item| item.address == *address && !item.signature.is_empty()) {
                continue;
            }

            let key =
                ChainTransDomain::get_key(self.ctx, address, &queue.chain_code, password, &None)
                    .await?;

            let res =
                tron::operations::multisig::TransactionOpt::sign_transaction(&queue.raw_data, key)?;
            let params = MultisigQueueRepo::build_signature(
                &queue.id,
                address,
                &res.signature,
                status,
                None,
            );

            MultisigQueueRepo::create_or_update_sign(&params, &core_pool).await?;
            result.push(params);

            if queue.chain_code == chain_code::SOLANA {
                let tx = BillRepo::build_signed_bill(
                    res.tx_hash,
                    address.clone(),
                    queue.chain_code.clone(),
                );
                let pool = self.ctx.core_pool()?;
                domain::bill::BillDomain::create_bill(&pool, tx).await?;
            }
        }

        Ok(result)
    }

    pub async fn exec_multisig_transaction(
        &self,
        queue_id: &str,
        password: String,
        fee_setting: Option<String>,
        request_resource_id: Option<String>,
    ) -> Result<String, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let queue = MultisigDomain::queue_by_id(queue_id, &pool).await?;
        MultisigQueueDomain::validate_queue(&queue, true)?;

        let signs = MultisigQueueRepo::get_signed_list(&core_pool, queue_id).await?;
        let signs_list = signs.get_order_sign_str();

        let coin = CoinDomain::get_coin_by_token_key_with_ctx(
            self.ctx,
            &queue.chain_code,
            queue.token_key(),
        )
        .await?;

        let transfer_amount = wallet_utils::unit::convert_to_u256(&queue.value, coin.decimals)?;

        let bill_kind = BillKind::try_from(queue.transfer_type)?;

        let instance =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &queue.chain_code)
                .await?;
        let tx_resp = match instance {
            MultisigAdapter::Ethereum(chain) => {
                let multisig_account =
                    MultisigDomain::account_by_address(&queue.from_addr, true, &pool).await?;

                let signatures = signs_list.join("");
                let params = eth::operations::MultisigTransferOpt::new(
                    &queue.from_addr,
                    &queue.to_addr,
                    alloy::primitives::U256::default(),
                )?
                .exec_params(
                    &multisig_account.initiator_addr,
                    queue.raw_data,
                    signatures,
                )?;

                let key = crate::domain::account::open_subpk_with_password(
                    self.ctx,
                    &queue.chain_code,
                    &multisig_account.initiator_addr,
                    &password,
                )
                .await?;

                let fee_setting = if let Some(fee_setting) = fee_setting {
                    crate::domain::chain::pare_fee_setting(&fee_setting)?
                } else {
                    return Err(crate::error::service::ServiceError::Parameter(
                        "empty fee setting".to_string(),
                    ));
                };

                // check transaction fee
                let balance = chain.balance(&multisig_account.initiator_addr, None).await?;

                let fee = fee_setting.transaction_fee();
                if balance < fee {
                    return Err(crate::error::service::ServiceError::Business(
                        crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::InsufficientFeeBalance,
                        ),
                    ));
                }

                let tx_hash = chain.exec_transaction(params, fee_setting, key, None).await?;
                TransferResp::new(tx_hash, unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?)
            }
            MultisigAdapter::BitCoin(chain) => {
                let account =
                    MultisigDomain::account_by_address(&queue.from_addr, true, &pool).await?;

                // 如果是p2tr-sh地址类型需要单独处理签名顺序问题
                let sign = if account.address_type == "p2tr-sh" {
                    let member =
                        MultisigMemberRepo::find_records_by_id(&core_pool, &account.id).await?;
                    member.sign_order(&signs.0)
                } else {
                    signs_list.clone()
                };

                let params = btc::operations::multisig::MultisigTransactionOpt::new(
                    account.address,
                    queue.value.clone(),
                    &account.salt,
                    &queue.raw_data,
                    &account.address_type,
                )?;

                let tx = chain
                    .exec_multisig_tx(params, sign, account.authority_addr)
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?;

                TransferResp::new(tx.tx_hash, tx.fee.to_string())
            }
            MultisigAdapter::Solana(chain) => {
                let multisig_account =
                    MultisigDomain::account_by_address(&queue.from_addr, true, &pool).await?;

                let key = crate::domain::account::open_subpk_with_password(
                    self.ctx,
                    &queue.chain_code,
                    &multisig_account.initiator_addr,
                    &password,
                )
                .await?;

                let params = sol::operations::multisig::transfer::ExecMultisigOpt::new(
                    &multisig_account.initiator_addr,
                    queue.raw_data,
                )?;

                let instructions = params.instructions().await?;

                // check transaction_fee
                let mut fee = chain.estimate_fee_v1(&instructions, &params).await?;
                let token = queue.token_addr.to_chain_token_option();
                ChainTransDomain::sol_priority_fee(&mut fee, token.as_ref(), 200_000);

                let balance = chain.balance(&multisig_account.initiator_addr, None).await?;
                ChainTransDomain::check_sol_transaction_fee(balance, fee.original_fee())?;

                let fees = fee.transaction_fee().to_string();

                let tx_hash =
                    chain.exec_transaction(params, key, Some(fee), instructions, 0).await?;

                TransferResp::new(tx_hash, fees)
            }
            MultisigAdapter::Tron(chain) => {
                // check balance
                let params =
                    tron::operations::multisig::TransactionOpt::data_from_str(&queue.raw_data)?;
                let provider = chain.get_provider();

                let transfer_balance = chain
                    .balance(&queue.from_addr, queue.token_key().to_chain_token_option())
                    .await?;

                // 根据交易类型来判断是否需要将amount 进行验证
                let transfer_amount = if bill_kind.out_transfer_type() {
                    transfer_amount
                } else {
                    alloy::primitives::U256::ZERO
                };
                if transfer_balance < transfer_amount {
                    return Err(crate::error::service::ServiceError::Business(
                        crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                                crate::error::business::chain::InsufficientBalanceDetail::new()
                                    .from_addr(queue.from_addr.clone())
                                    .to_addr(queue.to_addr.clone())
                                    .chain_code(queue.chain_code.clone())
                                    .token_addr(
                                        queue
                                            .token_key()
                                            .to_chain_token_option()
                                            .unwrap_or_default(),
                                    )
                                    .value(transfer_amount.to_string())
                                    .balance(transfer_balance.to_string())
                                    .need(transfer_amount.to_string())
                                    .reason("token balance is insufficient for multisig tron transaction"),
                            ),
                        ),
                    ));
                }

                let account = provider.account_info(&queue.from_addr).await?;
                let consumer = if let Some(token) = queue.token_key().to_chain_token_option() {
                    let memo = (!queue.notes.is_empty()).then(|| queue.notes.clone());

                    let value = unit::convert_to_u256(&queue.value, coin.decimals)?;
                    let transfer_params = tron::operations::transfer::ContractTransferOpt::new(
                        &token,
                        &queue.from_addr,
                        &queue.to_addr,
                        value,
                        memo,
                    )?;
                    let constant = transfer_params.constant_contract(provider).await?;

                    let consumer = provider
                        .contract_fee(
                            constant,
                            signs_list.len() as u8,
                            &transfer_params.owner_address,
                        )
                        .await?;
                    // check transaction fee
                    if account.balance < consumer.transaction_fee_i64() {
                        return Err(crate::error::service::ServiceError::Business(
                            crate::error::business::BusinessError::Chain(
                                crate::error::business::chain::ChainError::InsufficientFeeBalance,
                            ),
                        ));
                    }

                    consumer
                } else {
                    let to = (!queue.to_addr.is_empty()).then_some(queue.to_addr.as_str());

                    let mut consumer = provider
                        .transfer_fee(
                            &queue.from_addr,
                            to,
                            &params.raw_data_hex,
                            signs_list.len() as u8,
                        )
                        .await?;

                    if queue.transfer_type == BillKind::UpdatePermission.to_i8() {
                        consumer.set_extra_fee(100 * tron::consts::TRX_VALUE);
                    }

                    let value = transfer_amount.to::<i64>();
                    if account.balance < consumer.transaction_fee_i64() + value {
                        return Err(crate::error::service::ServiceError::Business(
                            crate::error::business::BusinessError::Chain(
                                crate::error::business::chain::ChainError::InsufficientFeeBalance,
                            ),
                        ));
                    }

                    consumer
                };

                let bill_consumer = BillResourceConsume::new_tron(
                    consumer.bandwidth.consumer as u64,
                    consumer.get_energy(),
                );
                let tx_hash = chain.exec_multisig_transaction(params, signs_list).await?;

                let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
                resp.with_consumer(bill_consumer);
                resp
            }
            _ => return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                        instance.to_string(),
                    ),
                ),
            )),
        };

        // 创建本地pending 交易
        let tx = BillRepo::build_bill(
            tx_resp.tx_hash.clone(),
            queue.from_addr,
            queue.to_addr,
            queue.value.parse().unwrap_or_default(),
            queue.chain_code,
            queue.symbol,
            true,
            bill_kind,
            queue.notes.clone(),
        )
        .with_token(queue.token_addr.as_db_str())
        .with_queue_id(queue_id)
        .with_resource_consume(&tx_resp.resource_consume()?)
        .with_transaction_fee(&tx_resp.fee);

        let pool = self.ctx.core_pool()?;
        domain::bill::BillDomain::create_bill(&pool, tx).await?;

        // sync status and tx_hash
        MultisigQueueRepo::update_status_and_hash(
            queue_id,
            MultisigQueueStatus::InConfirmation,
            &tx_resp.tx_hash,
            &core_pool,
        )
        .await?;

        let raw_data =
            MultisigQueueRepo::multisig_queue_data(queue_id, core_pool).await?.to_string()?;
        // 上报后端
        let req = SignedTranUpdateHashReq {
            withdraw_id: queue_id.to_string(),
            hash: tx_resp.tx_hash.clone(),
            remark: queue.notes,
            raw_data,
        };
        TaskQueueDomain::send_or_to_queue_with_ctx(
            self.ctx,
            req,
            endpoint::multisig::SIGNED_TRAN_UPDATE_TRANS_HASH,
        )
        .await?;

        // if let Err(e) = backend.signed_tran_update_trans_hash(cryptor, &req).await {
        //     tracing::error!("report signed tran update  add to task{}", e);
        //     let task = Task::BackendApi(BackendApiTask::BackendApi(BackendApiTaskData {
        //         endpoint: endpoint::multisig::SIGNED_TRAN_UPDATE_TRANS_HASH.to_string(),
        //         body: serde_func::serde_to_value(&req)?,
        //     }));
        //     Tasks::new().push(task).send().await?;
        // }

        // 回收资源
        if let Some(request_id) = request_resource_id {
            let backend = self.ctx.get_global_backend_api();
            let _rs = backend.delegate_complete(&request_id).await;
        }

        Ok(tx_resp.tx_hash)
    }

    pub async fn check_ongoing_queue(
        &self,
        chain_code: String,
        address: String,
    ) -> Result<Option<QueueInfo>, crate::error::service::ServiceError> {
        if chain_code.as_str() == chain_code::ETHEREUM
            || chain_code.as_str() == chain_code::BNB
            || chain_code.as_str() == chain_code::BTC
        {
            let pool = self.ctx.get_global_sqlite_pool()?;
            let core_pool = CoreDbPool::new(pool.clone());

            let rs = MultisigQueueRepo::ongoing_queue(&chain_code, &address, &core_pool)
                .await?
                .map(QueueInfo::from);
            Ok(rs)
        } else {
            Ok(None)
        }
    }

    // cancel multisig queue
    pub async fn cancel_queue(
        &self,
        queue_id: String,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let queue = MultisigDomain::queue_by_id(&queue_id, &pool).await?;

        // check status
        if !queue.can_cancel() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigQueue(
                    crate::error::business::multisig_queue::MultisigQueueError::CannotCancel,
                ),
            ));
        };

        // update status to fail
        MultisigQueueRepo::update_fail(&core_pool, &queue_id, fail_reason::CANCEL).await?;

        // report to backend ,if error rollback status
        let raw_data =
            MultisigQueueRepo::multisig_queue_data(&queue_id, core_pool).await?.to_string()?;
        let backend = self.ctx.get_global_backend_api();

        if let Err(e) = backend.signed_trans_cancel(&queue_id, raw_data).await {
            tracing::error!("cancel queue[{}] upload fail roolback err:{}", queue_id, e);
            MultisigQueueRepo::rollback_update_fail(
                &CoreDbPool::new(pool.clone()),
                &queue_id,
                queue.status,
            )
            .await?;
        }

        Ok(())
    }
}
