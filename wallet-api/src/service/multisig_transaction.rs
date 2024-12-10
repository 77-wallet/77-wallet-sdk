use crate::domain::chain::adapter::MultisigAdapter;
use crate::domain::chain::TransferResp;
use crate::domain::task_queue::{CommonTask, Task, Tasks};
use crate::mqtt::payload::incoming::transaction::{
    MultiSignTransAccept, MultiSignTransAcceptCompleteMsgBody,
};
use crate::response_vo::multisig_account::QueueInfo;
use crate::response_vo::MultisigQueueFeeParams;
use crate::response_vo::{multisig_transaction::MultisigQueueInfoVo, transaction::TransferParams};
use crate::{domain, response_vo};
use serde_json::json;
use wallet_chain_interact::sol::operations::SolInstructionOperation;
use wallet_chain_interact::tron::operations::TronConstantOperation as _;
use wallet_chain_interact::{btc, eth, sol, tron, BillResourceConsume};
use wallet_database::dao::multisig_member::MultisigMemberDaoV1;
use wallet_database::dao::multisig_queue::MultisigQueueDaoV1;
use wallet_database::entities::bill::{BillKind, NewBillEntity};
use wallet_database::entities::multisig_account::MultisigAccountEntity;
use wallet_database::entities::multisig_queue::{
    fail_reason, MultisigQueueEntity, MultisigQueueStatus, NewMultisigQueueEntity, QueueTaskEntity,
};
use wallet_database::entities::multisig_signatures::{MultisigSignatureStatus, NewSignatureEntity};
use wallet_database::pagination::Pagination;
use wallet_database::repositories::multisig_queue::MultisigQueueRepo;
use wallet_transport_backend::consts::endpoint;
use wallet_transport_backend::request::{
    SignedTranAcceptReq, SignedTranCreateReq, SignedTranUpdateHashReq,
};
use wallet_types::constant::chain_code;
use wallet_utils::{serde_func, unit};

pub struct MultisigTransactionService;

impl MultisigTransactionService {
    pub async fn create_queue_fee(
        req_params: MultisigQueueFeeParams,
    ) -> Result<response_vo::EstimateFeeResp, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let account =
            domain::multisig::MultisigDomain::account_by_address(&req_params.from, true, &pool)
                .await?;

        let assets = domain::chain::transaction::ChainTransaction::assets(
            &req_params.chain_code,
            &req_params.symbol,
            &req_params.from,
        )
        .await?;

        let main_coin =
            domain::chain::transaction::ChainTransaction::main_coin(&assets.chain_code).await?;

        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(&account.chain_code)
                .await?;

        let res = adapter
            .build_multisig_fee(
                &req_params,
                &account,
                assets.decimals,
                assets.token_address(),
                &main_coin.symbol,
            )
            .await?;
        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), res);

        Ok(fee_resp)
    }

    /// Creates a new multisig transaction queue.
    pub async fn create_multisig_queue(
        req_params: TransferParams,
    ) -> Result<String, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let queue_repo = MultisigQueueRepo::new(pool.clone());

        let account =
            domain::multisig::MultisigDomain::account_by_address(&req_params.from, true, &pool)
                .await?;
        domain::multisig::account::MultisigDomain::validate_queue(&account)?;

        let assets = domain::chain::transaction::ChainTransaction::assets(
            &req_params.chain_code,
            &req_params.symbol,
            &req_params.from,
        )
        .await?;

        let key = crate::domain::account::open_account_pk_with_password(
            &account.chain_code,
            &account.initiator_addr,
            &req_params.password,
        )
        .await?;
        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(&account.chain_code)
                .await?;
        let rs = adapter
            .build_multisig_tx(
                &req_params,
                &account,
                assets.decimals,
                assets.token_address(),
                key,
            )
            .await?;

        let mut queue_params = NewMultisigQueueEntity::from(&req_params)
            .with_msg_hash(&rs.tx_hash)
            .with_raw_data(&rs.raw_data)
            .with_token(assets.token_address())
            .set_id();
        queue_params.account_id = account.id.clone();
        let now = wallet_utils::time::now().timestamp();
        queue_params.expiration = queue_params.expiration * 3600 + now;
        let chain_code = &queue_params.chain_code;

        if req_params.chain_code != chain_code::SOLANA {
            // signed once
            let mut members = queue_repo.self_member_account_id(&account.id).await?;
            members.prioritize_by_address(&account.initiator_addr);

            // sign num
            let sign_num = members.0.len().min(account.threshold as usize);
            for i in 0..sign_num {
                let member = members.0.get(i).unwrap();
                let key = crate::domain::account::open_account_pk_with_password(
                    chain_code,
                    &member.address,
                    &req_params.password,
                )
                .await?;

                let sign_result = adapter
                    .sign_multisig_tx(&account, &member.address, key, &rs.raw_data)
                    .await?;
                let sign = NewSignatureEntity::new(
                    &queue_params.id,
                    &member.address,
                    &sign_result.signature,
                    MultisigSignatureStatus::Approved,
                );
                queue_params.signatures.push(sign);
            }
        }

        let signatures = queue_params.signatures.clone();
        queue_params.status =
            MultisigQueueEntity::compute_status(signatures.len(), account.threshold as usize);

        // write multisig queue data to local database
        let res =
            MultisigQueueRepo::create_queue_with_sign(pool.clone(), &mut queue_params).await?;

        // 上报后端
        let withdraw_id = res.id.clone();
        let mut sync_params = MultiSignTransAccept::from(res);
        sync_params.signatures = signatures;

        let raw_data = MultisigQueueRepo::multisig_queue_data(&withdraw_id, pool)
            .await?
            .to_string()?;
        let req = SignedTranCreateReq {
            withdraw_id,
            address: req_params.from,
            chain_code: req_params.chain_code,
            tx_str: wallet_utils::serde_func::serde_to_string(&sync_params)?,
            raw_data,
        };

        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData {
                    endpoint: endpoint::multisig::SIGNED_TRAN_CREATE.to_string(),
                    body: serde_func::serde_to_value(&req)?,
                },
            ));
        Tasks::new().push(task).send().await?;

        Ok(rs.tx_hash)
    }

    pub async fn multisig_queue_list(
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigQueueInfoVo>, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let queue_repo = MultisigQueueRepo::new(pool.clone());

        // 先处理过期的交易
        let _ = MultisigQueueDaoV1::update_expired_queue(pool.as_ref()).await;

        let mut lists = queue_repo
            .queue_list(from, chain_code, status, page, page_size)
            .await?;

        let mut task = Tasks::new();
        let mut data = vec![];
        // every queue signed num
        for item in lists.data.iter_mut() {
            // if queue status has exec , add to task query result
            if item.status == MultisigQueueStatus::InConfirmation.to_i8() {
                task = task.push(Task::Common(CommonTask::QueryQueueResult(
                    QueueTaskEntity {
                        id: item.id.clone(),
                        status: item.status,
                    },
                )));
            }

            let signature =
                MultisigQueueRepo::member_signed_result(&item.account_id, &item.id, pool.clone())
                    .await?;

            let sign_num: i64 = signature
                .iter()
                .filter_map(|sig| if sig.singed != 0 { Some(1) } else { None })
                .sum();
            item.sign_num = Some(sign_num);

            data.push(MultisigQueueInfoVo {
                queue: item.clone(),
                signature,
            });
        }

        task.send().await?;

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
        queue_id: &str,
    ) -> Result<MultisigQueueInfoVo, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let queue_repo = MultisigQueueRepo::new(pool.clone());

        let mut queue = queue_repo.find_by_id_with_account(queue_id).await?.ok_or(
            crate::BusinessError::MultisigQueue(crate::MultisigQueueError::NotFound),
        )?;

        let signature =
            MultisigQueueRepo::member_signed_result(&queue.account_id, queue_id, pool).await?;

        let sign_num: i64 = signature
            .iter()
            .filter_map(|sig| if sig.singed == 1 { Some(1) } else { None })
            .sum();
        queue.sign_num = Some(sign_num);

        Ok(MultisigQueueInfoVo { queue, signature })
    }

    pub async fn sign_fee(
        queue_id: String,
        address: String,
    ) -> Result<response_vo::EstimateFeeResp, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let queue = domain::multisig::MultisigDomain::queue_by_id(&queue_id, &pool).await?;
        let multisig_account =
            domain::multisig::MultisigDomain::account_by_address(&queue.from_addr, true, &pool)
                .await?;

        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(&queue.chain_code)
                .await?;

        let main_coin =
            domain::chain::transaction::ChainTransaction::main_coin(&queue.chain_code).await?;

        let res = adapter
            .sign_fee(
                &multisig_account,
                &address,
                &queue.raw_data,
                &main_coin.symbol,
            )
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), res);
        Ok(fee_resp)
    }

    // multisig sign message
    pub async fn sign_multisig_transaction(
        queue_id: &str,
        status: i32,
        password: &str,
        address: Option<String>,
    ) -> Result<(), crate::ServiceError> {
        let status = MultisigSignatureStatus::try_from(status)
            .map_err(|e| crate::ServiceError::Parameter(e.to_string()))?;

        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = MultisigQueueRepo::new(pool.clone());

        let queue = domain::multisig::MultisigDomain::queue_by_id(queue_id, &pool).await?;
        domain::multisig::MultisigQueueDomain::validate_queue(&queue, false)?;

        let multisig_account =
            domain::multisig::MultisigDomain::account_by_address(&queue.from_addr, true, &pool)
                .await?;

        // 1.签名
        let signed_res =
            Self::_sign_transaction(&queue, status, &multisig_account, &repo, password, address)
                .await?;
        let accept_address = signed_res.iter().map(|v| v.address.clone()).collect();

        // 同步签名的结果状态
        MultisigQueueRepo::sync_sign_status(
            queue_id,
            &multisig_account.id,
            multisig_account.threshold,
            pool.clone(),
        )
        .await?;

        // 3. 签名的结果发送给后端
        let params = signed_res
            .iter()
            .map(|i| i.into())
            .collect::<Vec<MultiSignTransAcceptCompleteMsgBody>>();

        let raw_data = MultisigQueueRepo::multisig_queue_data(queue_id, pool)
            .await?
            .to_string()?;
        let req = SignedTranAcceptReq {
            withdraw_id: queue_id.to_string(),
            tx_str: json!(params),
            accept_address,
            status: status.to_i8(),
            raw_data,
        };

        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData {
                    endpoint: endpoint::multisig::SIGNED_TRAN_ACCEPT.to_string(),
                    body: serde_func::serde_to_value(&req)?,
                },
            ));
        Tasks::new().push(task).send().await?;

        Ok(())
    }

    pub async fn multisig_transfer_fee(
        queue_id: &str,
    ) -> Result<response_vo::EstimateFeeResp, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = MultisigQueueRepo::new(pool.clone());

        let queue = domain::multisig::MultisigDomain::queue_by_id(queue_id, &pool).await?;

        let assets = domain::chain::transaction::ChainTransaction::assets(
            &queue.chain_code,
            &queue.symbol,
            &queue.from_addr,
        )
        .await?;

        // 签名数
        let signs = repo.get_signed_list(queue_id).await?;
        let sign_list = signs.get_order_sign_str();

        let instance =
            domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(&queue.chain_code)
                .await?;
        let main_coin =
            domain::chain::transaction::ChainTransaction::main_coin(&assets.chain_code).await?;

        let backend = crate::manager::Context::get_global_backend_api()?;
        let fee = instance
            .estimate_fee(
                &queue,
                &assets,
                backend,
                sign_list,
                main_coin.symbol.as_str(),
            )
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), fee);
        Ok(fee_resp)
    }

    // 执行签名
    pub async fn _sign_transaction(
        queue: &MultisigQueueEntity,
        status: MultisigSignatureStatus,
        multisig_account: &MultisigAccountEntity,
        repo: &MultisigQueueRepo,
        password: &str,
        address: Option<String>,
    ) -> Result<Vec<NewSignatureEntity>, crate::ServiceError> {
        let owner_address = match address {
            Some(address) => {
                vec![address]
            }
            None => repo
                .self_member_account_id(&queue.account_id)
                .await?
                .get_owner_str_vec(),
        };

        let mut result = vec![];
        match status {
            MultisigSignatureStatus::Rejected | MultisigSignatureStatus::UnSigned => {
                for address in owner_address {
                    let params = NewSignatureEntity::new(&queue.id, &address, "", status);
                    repo.create_or_update_sign(&params).await?;
                    result.push(params);
                }
            }
            MultisigSignatureStatus::Approved => {
                // 当前已签名的数量
                let sign_list = repo.get_signed_list(&queue.id).await?;

                // 批量执行签名
                let instance = domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(
                    &queue.chain_code,
                )
                .await?;

                let need_sign = (multisig_account.threshold as usize - sign_list.0.len()).max(0);
                if need_sign == 0 {
                    return Ok(result);
                }

                let mut signed_num = 0;

                for i in 0..owner_address.len() {
                    let address = owner_address.get(i).unwrap();
                    // filter already signed
                    if sign_list.contains_address(address) {
                        continue;
                    };

                    let key = crate::domain::account::open_account_pk_with_password(
                        &queue.chain_code,
                        address,
                        password,
                    )
                    .await?;
                    let rs = instance
                        .sign_multisig_tx(multisig_account, address, key, &queue.raw_data)
                        .await?;
                    let params = NewSignatureEntity::new(&queue.id, address, &rs.signature, status);

                    repo.create_or_update_sign(&params).await?;
                    result.push(params);

                    if queue.chain_code == "sol" {
                        let tx = NewBillEntity::new_signed_bill(
                            rs.tx_hash,
                            address.clone(),
                            queue.chain_code.clone(),
                            "SOL".to_string(),
                        );
                        domain::bill::BillDomain::create_bill(tx).await?;
                    }

                    signed_num += 1;
                    if signed_num >= need_sign {
                        break;
                    }
                }
            }
        };

        Ok(result)
    }

    pub async fn exec_multisig_transaction(
        queue_id: &str,
        password: String,
        fee_setting: Option<String>,
        request_resource_id: Option<String>,
    ) -> Result<String, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let queue = domain::multisig::MultisigDomain::queue_by_id(queue_id, &pool).await?;
        domain::multisig::MultisigQueueDomain::validate_queue(&queue, true)?;

        let repo = MultisigQueueRepo::new(pool.clone());

        let signs = repo.get_signed_list(queue_id).await?;
        let signs_list = signs.get_order_sign_str();

        let assets = domain::chain::transaction::ChainTransaction::assets(
            &queue.chain_code,
            &queue.symbol,
            &queue.from_addr,
        )
        .await?;
        let transfer_amcount = wallet_utils::unit::convert_to_u256(&queue.value, assets.decimals)?;

        let instance =
            domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(&queue.chain_code)
                .await?;
        let tx_resp = match instance {
            MultisigAdapter::Ethereum(chain) => {
                let multisig_account = domain::multisig::MultisigDomain::account_by_address(
                    &queue.from_addr,
                    true,
                    &pool,
                )
                .await?;

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

                let key = crate::domain::account::open_account_pk_with_password(
                    &queue.chain_code,
                    &multisig_account.initiator_addr,
                    &password,
                )
                .await?;

                let fee_setting = if let Some(fee_setting) = fee_setting {
                    crate::domain::chain::pare_fee_setting(&fee_setting)?
                } else {
                    return Err(crate::ServiceError::Parameter(
                        "empty fee setting".to_string(),
                    ));
                };

                // check transaction fee
                let balance = chain
                    .balance(&multisig_account.initiator_addr, None)
                    .await?;

                let fee = fee_setting.transaction_fee();
                if balance < fee {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientFeeBalance,
                    ))?;
                }

                let tx_hash = chain.exec_transaction(params, fee_setting, key).await?;
                TransferResp::new(
                    tx_hash,
                    unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?,
                )
            }
            MultisigAdapter::BitCoin(chain) => {
                let account = domain::multisig::MultisigDomain::account_by_address(
                    &queue.from_addr,
                    true,
                    &pool,
                )
                .await?;

                // 如果是p2tr-sh地址类型需要单独处理签名顺序问题
                let sign = if account.address_type == "p2tr-sh" {
                    let member =
                        MultisigMemberDaoV1::find_records_by_id(&account.id, pool.as_ref())
                            .await
                            .map_err(|e| crate::ServiceError::Database(e.into()))?;
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
                let tx_hash = chain
                    .exec_multisig_tx(params, sign, account.authority_addr)
                    .await
                    .map_err(domain::chain::transaction::ChainTransaction::handle_btc_fee_error)?;

                TransferResp::new(tx_hash, "0".to_string())
            }
            MultisigAdapter::Solana(chain) => {
                let multisig_account = domain::multisig::MultisigDomain::account_by_address(
                    &queue.from_addr,
                    true,
                    &pool,
                )
                .await?;

                let key = crate::domain::account::open_account_pk_with_password(
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
                let fee = chain.estimate_fee_v1(&instructions, &params).await?;
                let balance = chain
                    .balance(&multisig_account.initiator_addr, None)
                    .await?;
                domain::chain::transaction::ChainTransaction::check_sol_transaction_fee(
                    balance,
                    fee.original_fee(),
                )?;

                let tx_hash = chain
                    .exec_transaction(params, key, None, instructions, 0)
                    .await?;
                TransferResp::new(tx_hash, fee.transaction_fee().to_string())
            }
            MultisigAdapter::Tron(chain) => {
                // check balance
                let params =
                    tron::operations::multisig::TransactionOpt::data_from_str(&queue.raw_data)?;
                let provider = chain.get_provider();

                let transfer_balance = chain
                    .balance(&queue.from_addr, queue.token_address())
                    .await?;
                if transfer_balance < transfer_amcount {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }

                let account = provider.account_info(&queue.from_addr).await?;
                let consumer = if let Some(token) = queue.token_address() {
                    let assets = domain::chain::transaction::ChainTransaction::assets(
                        &queue.chain_code,
                        &queue.symbol,
                        &queue.from_addr,
                    )
                    .await?;

                    let memo = (!queue.notes.is_empty()).then(|| queue.notes.clone());

                    let value = unit::convert_to_u256(&queue.value, assets.decimals)?;
                    let transfer_params = tron::operations::transfer::ContractTransferOpt::new(
                        &token,
                        &queue.from_addr,
                        &queue.to_addr,
                        value,
                        memo,
                    )?;
                    let constant = transfer_params.constant_contract(provider).await?;

                    provider
                        .contract_fee(
                            constant,
                            signs_list.len() as u8,
                            &transfer_params.owner_address,
                        )
                        .await?
                } else {
                    provider
                        .transfer_fee(
                            &queue.from_addr,
                            Some(&queue.to_addr),
                            &params,
                            signs_list.len() as u8,
                        )
                        .await?
                };

                // check transaction fee
                if account.balance < consumer.transaction_fee_i64() {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientFeeBalance,
                    ))?;
                }
                let bill_consumer = BillResourceConsume::new_tron(
                    consumer.bandwidth.consumer as u64,
                    consumer.get_energy(),
                );
                let tx_hash = chain.exec_multisig_transaction(params, signs_list).await?;

                let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
                resp.with_consumer(bill_consumer);
                resp
            }
        };

        // 创建本地pending 交易
        let tx = NewBillEntity::new(
            tx_resp.tx_hash.clone(),
            queue.from_addr,
            queue.to_addr,
            queue.value.parse().unwrap(),
            queue.chain_code,
            queue.symbol,
            true,
            BillKind::Transfer,
            queue.notes.clone(),
        )
        .with_queue_id(queue_id)
        .with_resource_consume(&tx_resp.resource_consume()?)
        .with_transaction_fee(&tx_resp.fee);

        domain::bill::BillDomain::create_bill(tx).await?;

        // sync status and tx_hash
        repo.update_status_and_hash(
            queue_id,
            MultisigQueueStatus::InConfirmation,
            &tx_resp.tx_hash,
        )
        .await?;

        let raw_data = MultisigQueueRepo::multisig_queue_data(queue_id, pool)
            .await?
            .to_string()?;
        // 上报后端
        let req = SignedTranUpdateHashReq {
            withdraw_id: queue_id.to_string(),
            hash: tx_resp.tx_hash.clone(),
            remark: queue.notes,
            raw_data,
        };

        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData {
                    endpoint: endpoint::multisig::SIGNED_TRAN_UPDATE_TRANS_HASH.to_string(),
                    body: serde_func::serde_to_value(&req)?,
                },
            ));
        Tasks::new().push(task).send().await?;

        // 回收资源
        let backend = crate::manager::Context::get_global_backend_api()?;
        if let Some(request_id) = request_resource_id {
            let _rs = backend.delegate_complete(&request_id).await;
        }

        Ok(tx_resp.tx_hash)
    }

    pub async fn check_ongoing_queue(
        chain_code: String,
        address: String,
    ) -> Result<Option<QueueInfo>, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let queue_repo = MultisigQueueRepo::new(pool.clone());

        let rs = queue_repo
            .ongoing_queue(&chain_code, &address)
            .await?
            .and_then(|q| Some(QueueInfo::from(q)));
        Ok(rs)

        // if chain_code.as_str() == chain_code::ETHEREUM || chain_code.as_str() == chain_code::BNB {
        //     let pool = crate::manager::Context::get_global_sqlite_pool()?;
        //     let queue_repo = MultisigQueueRepo::new(pool.clone());

        //     let rs = queue_repo
        //         .ongoing_queue(&chain_code, &address)
        //         .await?
        //         .and_then(|q| Some(QueueInfo::from(q)));
        //     Ok(rs)
        // } else {
        //     Ok(None)
        // }
    }

    // cancel multisig queue
    pub async fn cancel_queue(queue_id: String) -> Result<(), crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;
        let queue = domain::multisig::MultisigDomain::queue_by_id(&queue_id, &pool).await?;

        // check status
        if !queue.can_cancel() {
            return Err(crate::BusinessError::MultisigQueue(
                crate::MultisigQueueError::CannotCancel,
            ))?;
        };

        // update status to fail
        MultisigQueueDaoV1::update_fail(&queue_id, fail_reason::CANCEL, pool.as_ref())
            .await
            .map_err(|e| crate::SystemError::Database(e.into()))?;

        // report to backend ,if error rollback status
        let raw_data = MultisigQueueRepo::multisig_queue_data(&queue_id, pool.clone())
            .await?
            .to_string()?;
        let backend = crate::Context::get_global_backend_api()?;
        if let Err(_e) = backend.signed_trans_cancel(&queue_id, raw_data).await {
            MultisigQueueDaoV1::rollback_update_fail(&queue_id, queue.status, pool.as_ref())
                .await
                .map_err(|e| crate::SystemError::Database(e.into()))?;
        }

        Ok(())
    }
}
