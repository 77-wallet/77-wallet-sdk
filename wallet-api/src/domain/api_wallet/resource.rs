use crate::{
    context::Context,
    domain::{
        api_wallet::{ApiChainAdapterFactory, account::ApiAccountDomain, adapter::tx::RawTx},
        chain::adapter::ChainAdapterFactory,
    },
    error::{
        business::{
            BusinessError,
            api_wallet::{ApiWalletError, account::AccountError, wallet::WalletError},
            chain::ChainError,
        },
        service::ServiceError,
    },
    messaging::notify::{
        FrontendNotifyEvent,
        event::NotifyEvent,
        other::{Process, TransactionProcessFrontend},
    },
    request::{
        stake::{
            BatchDelegate, BatchUnDelegate, DelegateReq, FreezeBalanceReq, UnDelegateReq,
            UnFreezeBalanceReq, VoteWitnessReq, WithdrawBalanceReq,
        },
        transaction::Signer,
    },
    response_vo::standard_wallet::stake::{
        BatchDelegateResp, BatchRes, DelegateResp, FreezeResp, ResourceResp, WithdrawUnfreezeResp,
    },
};
use wallet_chain_interact::{
    BillResourceConsume,
    tron::{
        self,
        operations::{
            RawTransactionParams, TronTxOperation,
            stake::{
                DelegateArgs, FreezeBalanceArgs, ResourceType, UnDelegateArgs, UnFreezeBalanceArgs,
                VoteWitnessArgs, WithdrawBalanceArgs, WithdrawUnfreezeArgs,
            },
        },
    },
    types::ChainPrivateKey,
};
use wallet_database::{
    entities::{api_account::ApiAccountEntity, api_wallet::ApiWalletType, bill::BillKind},
    repositories::api_wallet::{account::ApiAccountRepo, wallet::ApiWalletRepo},
};

pub(crate) struct ApiResourceDomain;

pub(crate) struct ApiResourceBroadcastOutcome {
    pub(crate) uid: Option<String>,
    pub(crate) resource_type: Option<ResourceType>,
    pub(crate) amount: Option<String>,
    pub(crate) tx_hash: String,
    pub(crate) owner_address: String,
    pub(crate) raw_tx: String,
    pub(crate) transaction_fee: String,
    pub(crate) resp: Option<FreezeResp>,
    pub(crate) delegate_resp: Option<DelegateResp>,
    pub(crate) withdraw_unfreeze_resp: Option<WithdrawUnfreezeResp>,
}

pub(crate) struct ApiWithdrawWalletAccountContext {
    pub(crate) uid: String,
    pub(crate) owner_address: String,
}

struct TempBuildTransaction {
    to: String,
    amount: i64,
    transaction_fee_sun: i64,
    transaction_fee: String,
    resource_consume: BillResourceConsume,
    raw_tx: RawTransactionParams,
}

pub(crate) struct ApiBatchResourceBroadcast {
    pub(crate) receiver_address: String,
    pub(crate) amount: i64,
    pub(crate) tx_hash: String,
    pub(crate) raw_tx: String,
    pub(crate) transaction_fee: String,
}

pub(crate) struct ApiBatchResourceOutcome {
    pub(crate) resp: BatchDelegateResp,
    pub(crate) broadcasts: Vec<ApiBatchResourceBroadcast>,
}

impl ApiResourceDomain {
    pub(crate) async fn stake_withdraw_wallet_resource(
        ctx: &'static Context,
        req: &FreezeBalanceReq,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;
        let frozen_balance_trx = Self::parse_amount_trx(req.frozen_balance)?;
        let resource_type = ResourceType::try_from(req.resource.as_str())?;
        let args = FreezeBalanceArgs::try_from(req)?;

        let bill_kind = match resource_type {
            ResourceType::BANDWIDTH => BillKind::FreezeBandwidth,
            ResourceType::ENERGY => BillKind::FreezeEnergy,
        };

        let mut outcome = Self::execute_tron_resource_operation(
            owner_ctx.owner_address,
            frozen_balance_trx,
            args,
            bill_kind,
            &req.signer,
            ctx,
        )
        .await?;
        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(resource_type);
        let resource_value =
            Self::resource_value(&outcome.owner_address, frozen_balance_trx, resource_type, ctx)
                .await?;
        let resource = ResourceResp::new(frozen_balance_trx, resource_type, resource_value);
        outcome.resp = Some(FreezeResp::new(
            outcome.owner_address.clone(),
            resource,
            outcome.tx_hash.clone(),
            bill_kind,
        ));
        Ok(outcome)
    }

    pub(crate) async fn unstake_withdraw_wallet_resource(
        ctx: &'static Context,
        req: &UnFreezeBalanceReq,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;
        let unfreeze_balance_trx = Self::parse_amount_trx(req.unfreeze_balance)?;
        let resource_type = ResourceType::try_from(req.resource.as_str())?;
        let args = UnFreezeBalanceArgs::try_from(req)?;

        let bill_kind = match resource_type {
            ResourceType::BANDWIDTH => BillKind::UnFreezeBandwidth,
            ResourceType::ENERGY => BillKind::UnFreezeEnergy,
        };

        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        let can_withdraw =
            chain.get_provider().can_withdraw_unfreeze_amount(&req.owner_address).await?;

        let mut outcome = Self::execute_tron_resource_operation(
            owner_ctx.owner_address,
            0,
            args,
            bill_kind,
            &req.signer,
            ctx,
        )
        .await?;
        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(resource_type);
        let resource_value =
            Self::resource_value(&outcome.owner_address, unfreeze_balance_trx, resource_type, ctx)
                .await?;
        let resource = ResourceResp::new(unfreeze_balance_trx, resource_type, resource_value);
        outcome.resp = Some(
            FreezeResp::new(
                outcome.owner_address.clone(),
                resource,
                outcome.tx_hash.clone(),
                bill_kind,
            )
            .expiration_at(wallet_utils::time::now_plus_days(14))
            .withdraw_amount(can_withdraw.to_sun()),
        );
        Ok(outcome)
    }

    pub(crate) async fn delegate_withdraw_wallet_resource(
        ctx: &'static Context,
        req: DelegateReq,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;
        let balance_trx = Self::parse_amount_trx(req.balance)?;
        let resource_type = ResourceType::try_from(req.resource.as_str())?;
        let bill_kind = Self::delegate_bill_kind(resource_type);
        let args = DelegateArgs::try_from(&req)?;
        let resource_value =
            Self::resource_value(&owner_ctx.owner_address, balance_trx, resource_type, ctx).await?;
        let mut outcome = Self::execute_tron_resource_operation(
            owner_ctx.owner_address,
            0,
            args,
            bill_kind,
            &req.signer,
            ctx,
        )
        .await?;
        let resource = ResourceResp::new(balance_trx, resource_type, resource_value);

        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(resource_type);
        outcome.amount = Some(balance_trx.to_string());
        outcome.delegate_resp =
            Some(DelegateResp::new_delegate(req, resource, bill_kind, outcome.tx_hash.clone()));
        Ok(outcome)
    }

    pub(crate) async fn undelegate_withdraw_wallet_resource(
        ctx: &'static Context,
        req: UnDelegateReq,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;
        let balance_trx = Self::parse_amount_trx(req.balance)?;
        let resource_type = ResourceType::try_from(req.resource.as_str())?;
        let bill_kind = Self::undelegate_bill_kind(resource_type);
        let args = UnDelegateArgs::try_from(&req)?;
        let resource_value =
            Self::resource_value(&owner_ctx.owner_address, balance_trx, resource_type, ctx).await?;
        let mut outcome = Self::execute_tron_resource_operation(
            owner_ctx.owner_address,
            0,
            args,
            bill_kind,
            &req.signer,
            ctx,
        )
        .await?;
        let resource = ResourceResp::new(balance_trx, resource_type, resource_value);

        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(resource_type);
        outcome.amount = Some(balance_trx.to_string());
        outcome.delegate_resp =
            Some(DelegateResp::new_undelegate(req, resource, bill_kind, outcome.tx_hash.clone()));
        Ok(outcome)
    }

    pub(crate) fn delegate_bill_kind(resource_type: ResourceType) -> BillKind {
        match resource_type {
            ResourceType::BANDWIDTH => BillKind::DelegateBandwidth,
            ResourceType::ENERGY => BillKind::DelegateEnergy,
        }
    }

    pub(crate) fn undelegate_bill_kind(resource_type: ResourceType) -> BillKind {
        match resource_type {
            ResourceType::BANDWIDTH => BillKind::UnDelegateBandwidth,
            ResourceType::ENERGY => BillKind::UnDelegateEnergy,
        }
    }

    pub(crate) fn batch_delegate_bill_kind(resource_type: ResourceType) -> BillKind {
        match resource_type {
            ResourceType::BANDWIDTH => BillKind::BatchDelegateBandwidth,
            ResourceType::ENERGY => BillKind::BatchDelegateEnergy,
        }
    }

    pub(crate) fn batch_undelegate_bill_kind(resource_type: ResourceType) -> BillKind {
        match resource_type {
            ResourceType::BANDWIDTH => BillKind::BatchUnDelegateBandwidth,
            ResourceType::ENERGY => BillKind::BatchUnDelegateEnergy,
        }
    }

    pub(crate) async fn batch_delegate_withdraw_wallet_resource(
        ctx: &'static Context,
        req: BatchDelegate,
    ) -> Result<ApiBatchResourceOutcome, ServiceError> {
        let resource_type = ResourceType::try_from(req.resource_type.as_str())?;

        for list in req.list.iter() {
            if list.value <= 0 {
                return Err(ServiceError::Business(BusinessError::Stake(
                    crate::error::business::stake::StakeError::DelegateLessThanMin,
                )));
            }
        }

        let bill_kind = Self::batch_delegate_bill_kind(resource_type);
        let args: Vec<DelegateArgs> = (&req).try_into()?;
        let amounts = req.list.iter().map(|item| item.value).collect();
        let amount = req.total();

        Self::process_batch(
            ctx,
            resource_type,
            &req.owner_address,
            bill_kind,
            args,
            amounts,
            amount,
            &req.signer,
        )
        .await
    }

    pub(crate) async fn batch_undelegate_withdraw_wallet_resource(
        ctx: &'static Context,
        req: BatchUnDelegate,
    ) -> Result<ApiBatchResourceOutcome, ServiceError> {
        let resource_type = ResourceType::try_from(req.resource_type.as_str())?;

        for list in req.list.iter() {
            if list.value <= 0 {
                return Err(ServiceError::Business(BusinessError::Stake(
                    crate::error::business::stake::StakeError::UnDelegateLessThanMin,
                )));
            }
        }

        let bill_kind = Self::batch_undelegate_bill_kind(resource_type);
        let args: Vec<UnDelegateArgs> = (&req).try_into()?;
        let amounts = req.list.iter().map(|item| item.value).collect();
        let amount = req.total();

        Self::process_batch(
            ctx,
            resource_type,
            &req.owner_address,
            bill_kind,
            args,
            amounts,
            amount,
            &req.signer,
        )
        .await
    }

    async fn get_key(
        ctx: &'static Context,
        from: &str,
        signer: &Option<Signer>,
    ) -> Result<ChainPrivateKey, ServiceError> {
        let signing_address = signer.as_ref().map(|signer| signer.address.as_str()).unwrap_or(from);
        ApiAccountDomain::get_private_key_with_ctx(ctx, signing_address, "tron").await
    }

    async fn batch_build_transaction<T>(
        ctx: &'static Context,
        chain: &wallet_chain_interact::tron::TronChain,
        from: &str,
        args: Vec<impl TronTxOperation<T>>,
        amounts: Vec<i64>,
        bill_kind: BillKind,
    ) -> Result<Vec<TempBuildTransaction>, ServiceError> {
        let mut result = vec![];
        for (i, (item, amount)) in args.into_iter().zip(amounts).enumerate() {
            let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new_with_num(
                bill_kind,
                (i + 1) as i64,
                Process::Building,
            ));
            FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
            let raw_tx = item.build_raw_transaction(&chain.provider).await?;
            let consumer =
                chain.get_provider().transfer_fee(from, None, &raw_tx.raw_data_hex, 1).await?;
            let resource_consume =
                BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0);
            result.push(TempBuildTransaction {
                to: item.get_to(),
                amount,
                transaction_fee_sun: consumer.transaction_fee_i64(),
                transaction_fee: consumer.transaction_fee(),
                resource_consume,
                raw_tx,
            });
        }
        Ok(result)
    }

    async fn batch_exec(
        ctx: &'static Context,
        chain: &wallet_chain_interact::tron::TronChain,
        key: ChainPrivateKey,
        bill_kind: BillKind,
        txs: Vec<TempBuildTransaction>,
    ) -> Result<(Vec<BatchRes>, Vec<String>, Vec<ApiBatchResourceBroadcast>), ServiceError> {
        let mut exec_res = vec![];
        let mut tx_hash = vec![];
        let mut broadcasts = vec![];
        let txs = txs
            .into_iter()
            .map(|mut item| {
                let sign = wallet_utils::sign::sign_tron(&item.raw_tx.tx_id, &key, None)?;
                item.raw_tx.signature.push(sign);
                Ok(item)
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        for (i, item) in txs.into_iter().enumerate() {
            let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new_with_num(
                bill_kind,
                (i + 1) as i64,
                Process::Broadcast,
            ));
            FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
            let TempBuildTransaction {
                to,
                amount,
                transaction_fee_sun: _,
                transaction_fee,
                resource_consume,
                raw_tx,
            } = item;
            let expected_hash = raw_tx.tx_id.clone();
            let persisted_raw = RawTx::Tron(raw_tx, resource_consume, transaction_fee.clone());
            let raw_tx = wallet_utils::serde_func::serde_to_string(&persisted_raw)?;
            let RawTx::Tron(broadcast_raw, ..) = persisted_raw else {
                unreachable!("batch resource operation builds tron tx")
            };
            match chain.get_provider().exec_raw_transaction(broadcast_raw).await {
                Ok(result) => {
                    if result.tx_id != expected_hash {
                        return Err(ServiceError::System(
                            crate::error::system::SystemError::Internal(
                                "api wallet batch resource tx_hash mismatch after broadcast"
                                    .to_string(),
                            ),
                        ));
                    }
                    exec_res.push(BatchRes { address: to.clone(), status: true });
                    tx_hash.push(result.tx_id.clone());
                    broadcasts.push(ApiBatchResourceBroadcast {
                        receiver_address: to,
                        amount,
                        tx_hash: result.tx_id,
                        raw_tx,
                        transaction_fee,
                    });
                }
                Err(_) => exec_res.push(BatchRes { address: to, status: false }),
            }
        }
        Ok((exec_res, tx_hash, broadcasts))
    }

    async fn process_batch<T>(
        ctx: &'static Context,
        resource_type: ResourceType,
        owner_address: &str,
        bill_kind: BillKind,
        args: Vec<impl TronTxOperation<T>>,
        amounts: Vec<i64>,
        amount: i64,
        signer: &Option<Signer>,
    ) -> Result<ApiBatchResourceOutcome, ServiceError>
    where
        T: Send + 'static,
    {
        Self::withdraw_wallet_account_context(ctx, owner_address).await?;
        let adapter = ApiChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        let chain = adapter.chain();
        let account_resource = chain.account_resource(owner_address).await?;
        let resource_value = account_resource.resource_value(resource_type, amount)?;
        let txs =
            Self::batch_build_transaction(ctx, chain, owner_address, args, amounts, bill_kind)
                .await?;
        let balance = chain.balance(owner_address, None).await?;
        let total_fee = txs.iter().map(|item| item.transaction_fee_sun).sum::<i64>();
        if balance.to::<i64>() < total_fee {
            return Err(ServiceError::Business(BusinessError::Chain(
                ChainError::InsufficientFeeBalance,
            )));
        }
        let key = Self::get_key(ctx, owner_address, signer).await?;
        let (result, hashes, broadcasts) =
            Self::batch_exec(ctx, chain, key, bill_kind, txs).await?;
        let resp = BatchDelegateResp::new(
            owner_address.to_string(),
            (result, hashes),
            ResourceResp::new(amount, resource_type, resource_value),
            bill_kind,
        );
        Ok(ApiBatchResourceOutcome { resp, broadcasts })
    }

    pub(crate) async fn withdraw_wallet_votes(
        ctx: &'static Context,
        req: VoteWitnessReq,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;
        let args = VoteWitnessArgs::try_from(&req)?;
        let mut outcome = Self::execute_tron_resource_operation(
            req.owner_address.clone(),
            0,
            args,
            BillKind::Vote,
            &req.signer,
            ctx,
        )
        .await?;
        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(ResourceType::BANDWIDTH);
        outcome.amount = Some(req.get_votes().to_string());
        Ok(outcome)
    }

    pub(crate) async fn withdraw_wallet_claim_votes_rewards(
        ctx: &'static Context,
        req: WithdrawBalanceReq,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        let value = chain.get_provider().get_reward(&req.owner_address).await?.to_sun();
        if value < 0.0 {
            return Err(ServiceError::Business(BusinessError::Chain(ChainError::NoRewardClaim)));
        }
        let mut args = WithdrawBalanceArgs::try_from(&req)?;
        args.value = Some(value);
        let mut outcome = Self::execute_tron_resource_operation(
            req.owner_address.clone(),
            0,
            args,
            BillKind::WithdrawReward,
            &req.signer,
            ctx,
        )
        .await?;
        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(ResourceType::BANDWIDTH);
        outcome.amount = Some(value.to_string());
        Ok(outcome)
    }

    pub(crate) async fn withdraw_wallet_unfreeze(
        ctx: &'static Context,
        req: WithdrawBalanceReq,
        withdraw_amount_sun: i64,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError> {
        let owner_ctx = Self::withdraw_wallet_account_context(ctx, &req.owner_address).await?;

        let args = WithdrawUnfreezeArgs {
            owner_address: req.owner_address.clone(),
            permission_id: req.signer.clone().map(|s| s.permission_id),
        };
        let mut outcome = Self::execute_tron_resource_operation(
            req.owner_address.clone(),
            0,
            args,
            BillKind::WithdrawUnFreeze,
            &req.signer,
            ctx,
        )
        .await?;
        outcome.uid = Some(owner_ctx.uid);
        outcome.resource_type = Some(ResourceType::BANDWIDTH);
        outcome.amount = Some(withdraw_amount_sun.to_string());
        outcome.withdraw_unfreeze_resp = Some(WithdrawUnfreezeResp {
            amount: withdraw_amount_sun,
            owner_address: outcome.owner_address.clone(),
            tx_hash: outcome.tx_hash.clone(),
        });
        Ok(outcome)
    }

    pub(crate) async fn withdraw_wallet_unfreeze_amount(
        ctx: &'static Context,
        owner_address: &str,
    ) -> Result<i64, ServiceError> {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        let can_withdraw = chain.get_provider().can_withdraw_unfreeze_amount(owner_address).await?;
        if can_withdraw.amount <= 0 {
            return Err(ServiceError::Business(BusinessError::Stake(
                crate::error::business::stake::StakeError::NoWithdrawableAmount,
            )));
        }
        Ok(can_withdraw.to_sun())
    }

    pub(crate) async fn withdraw_wallet_account_context(
        ctx: &'static Context,
        owner_address: &str,
    ) -> Result<ApiWithdrawWalletAccountContext, ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let account = ApiAccountRepo::find_one_by_address_chain_code(owner_address, "tron", &pool)
            .await?
            .ok_or(ServiceError::Business(
                ApiWalletError::Account(AccountError::NotFound).into(),
            ))?;

        Self::account_context_from_entity(account)
    }

    pub(crate) async fn withdraw_wallet_address(
        ctx: &'static Context,
        withdraw_wallet_uid: &str,
    ) -> Result<String, ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let wallet = ApiWalletRepo::find_by_uid(&pool, withdraw_wallet_uid)
            .await?
            .ok_or(ServiceError::Business(ApiWalletError::Wallet(WalletError::NotFound).into()))?;

        if wallet.api_wallet_type != ApiWalletType::Withdrawal {
            return Err(ServiceError::Business(
                ApiWalletError::Wallet(WalletError::WithdrawalWalletNotUsed).into(),
            ));
        }

        let tron_accounts =
            ApiAccountRepo::find_all_by_wallet_address_index(&pool, &wallet.address, "tron", 1)
                .await?;
        tron_accounts.into_iter().next().map(|account| account.address).ok_or_else(|| {
            ServiceError::Parameter(format!(
                "withdraw wallet tron account not found: uid={withdraw_wallet_uid}"
            ))
        })
    }

    fn account_context_from_entity(
        account: ApiAccountEntity,
    ) -> Result<ApiWithdrawWalletAccountContext, ServiceError> {
        if account.api_wallet_type != ApiWalletType::Withdrawal {
            return Err(ServiceError::Business(
                ApiWalletError::Wallet(WalletError::WithdrawalWalletNotUsed).into(),
            ));
        }

        Ok(ApiWithdrawWalletAccountContext { uid: account.uid, owner_address: account.address })
    }

    fn parse_amount_trx(amount: i64) -> Result<i64, ServiceError> {
        if amount <= 0 {
            return Err(ServiceError::Parameter(
                "api wallet resource amount must be positive".to_string(),
            ));
        }
        Ok(amount)
    }

    async fn execute_tron_resource_operation<T>(
        owner_address: String,
        stake_amount_trx: i64,
        args: impl TronTxOperation<T>,
        bill_kind: BillKind,
        signer: &Option<Signer>,
        ctx: &'static Context,
    ) -> Result<ApiResourceBroadcastOutcome, ServiceError>
    where
        T: Send + 'static,
    {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;

        let raw_tx = args.build_raw_transaction(&chain.provider).await?;
        let balance = chain.balance(&owner_address, None).await?;
        let consumer = chain
            .get_provider()
            .transfer_fee(&owner_address, None, &raw_tx.raw_data_hex, 1)
            .await?;
        let need_sun = Self::required_balance_sun(consumer.transaction_fee_i64(), stake_amount_trx);
        if balance.to::<i64>() < need_sun {
            return Err(ServiceError::Business(BusinessError::Chain(
                ChainError::InsufficientBalance(Default::default()),
            )));
        }

        let signing_address =
            signer.as_ref().map(|signer| signer.address.as_str()).unwrap_or(&owner_address);
        let private_key =
            ApiAccountDomain::get_private_key_with_ctx(ctx, signing_address, "tron").await?;
        let resource_consume =
            BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0).to_json_str()?;
        let sign = wallet_utils::sign::sign_tron(&raw_tx.tx_id, &private_key, None)?;
        let mut raw_tx = raw_tx;
        raw_tx.signature.push(sign);
        let tx_hash = raw_tx.tx_id.clone();
        let transaction_fee = consumer.transaction_fee();
        let raw_tx = RawTx::Tron(
            raw_tx,
            BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0),
            transaction_fee.clone(),
        );
        let raw_tx_str = wallet_utils::serde_func::serde_to_string(&raw_tx)?;
        let RawTx::Tron(raw_tx, ..) = raw_tx else {
            unreachable!("resource operation builds tron tx")
        };
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            bill_kind,
            Process::Broadcast,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        let broadcast_hash = chain.get_provider().exec_raw_transaction(raw_tx).await?.tx_id;
        if broadcast_hash != tx_hash {
            return Err(ServiceError::System(crate::error::system::SystemError::Internal(
                "api wallet resource tx_hash mismatch after broadcast".to_string(),
            )));
        }
        tracing::info!(
            owner_address = %owner_address,
            tx_hash = %tx_hash,
            transaction_fee = %transaction_fee,
            resource_consume = %resource_consume,
            "API wallet foreground resource operation broadcasted"
        );

        Ok(ApiResourceBroadcastOutcome {
            uid: None,
            resource_type: None,
            amount: None,
            tx_hash,
            owner_address,
            raw_tx: raw_tx_str,
            transaction_fee,
            resp: None,
            delegate_resp: None,
            withdraw_unfreeze_resp: None,
        })
    }

    async fn resource_value(
        owner_address: &str,
        amount: i64,
        resource_type: ResourceType,
        ctx: &'static Context,
    ) -> Result<f64, ServiceError> {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        let resource = chain.account_resource(owner_address).await?;
        Ok(resource.resource_value(resource_type, amount)?)
    }

    fn required_balance_sun(transaction_fee_sun: i64, stake_amount_trx: i64) -> i64 {
        transaction_fee_sun.saturating_add(stake_amount_trx.saturating_mul(tron::consts::TRX_VALUE))
    }
}

#[cfg(test)]
mod tests {
    use super::ApiResourceDomain;
    use wallet_chain_interact::tron::operations::stake::ResourceType;
    use wallet_database::entities::{
        api_account::ApiAccountEntity, api_wallet::ApiWalletType, bill::BillKind,
    };

    #[test]
    fn api_resource_amount_requires_positive_integer_trx() {
        assert_eq!(ApiResourceDomain::parse_amount_trx(1000).unwrap(), 1000);
        assert!(ApiResourceDomain::parse_amount_trx(0).is_err());
        assert!(ApiResourceDomain::parse_amount_trx(-1).is_err());
    }

    #[test]
    fn api_resource_stake_balance_check_uses_trx_unit() {
        assert_eq!(
            ApiResourceDomain::required_balance_sun(345, 2),
            2 * wallet_chain_interact::tron::consts::TRX_VALUE + 345
        );
    }

    #[test]
    fn api_withdraw_wallet_delegate_uses_delegate_bill_kinds() {
        assert_eq!(
            ApiResourceDomain::delegate_bill_kind(ResourceType::BANDWIDTH) as u8,
            BillKind::DelegateBandwidth as u8
        );
        assert_eq!(
            ApiResourceDomain::delegate_bill_kind(ResourceType::ENERGY) as u8,
            BillKind::DelegateEnergy as u8
        );
    }

    #[test]
    fn api_withdraw_wallet_undelegate_uses_reclaim_bill_kinds() {
        assert_eq!(
            ApiResourceDomain::undelegate_bill_kind(ResourceType::BANDWIDTH) as u8,
            BillKind::UnDelegateBandwidth as u8
        );
        assert_eq!(
            ApiResourceDomain::undelegate_bill_kind(ResourceType::ENERGY) as u8,
            BillKind::UnDelegateEnergy as u8
        );
    }

    #[test]
    fn api_withdraw_wallet_batch_delegate_uses_batch_bill_kinds() {
        assert_eq!(
            ApiResourceDomain::batch_delegate_bill_kind(ResourceType::BANDWIDTH) as u8,
            BillKind::BatchDelegateBandwidth as u8
        );
        assert_eq!(
            ApiResourceDomain::batch_delegate_bill_kind(ResourceType::ENERGY) as u8,
            BillKind::BatchDelegateEnergy as u8
        );
    }

    #[test]
    fn api_withdraw_wallet_batch_reclaim_uses_batch_bill_kinds() {
        assert_eq!(
            ApiResourceDomain::batch_undelegate_bill_kind(ResourceType::BANDWIDTH) as u8,
            BillKind::BatchUnDelegateBandwidth as u8
        );
        assert_eq!(
            ApiResourceDomain::batch_undelegate_bill_kind(ResourceType::ENERGY) as u8,
            BillKind::BatchUnDelegateEnergy as u8
        );
    }

    #[test]
    fn api_withdraw_wallet_account_context_accepts_withdrawal_account() {
        let account = test_account(ApiWalletType::Withdrawal);
        let ctx = ApiResourceDomain::account_context_from_entity(account).unwrap();

        assert_eq!(ctx.uid, "withdraw-uid");
        assert_eq!(ctx.owner_address, "TWithdrawOwner");
    }

    #[test]
    fn api_withdraw_wallet_account_context_rejects_non_withdrawal_account() {
        let account = test_account(ApiWalletType::SubAccount);

        assert!(ApiResourceDomain::account_context_from_entity(account).is_err());
    }

    fn test_account(api_wallet_type: ApiWalletType) -> ApiAccountEntity {
        ApiAccountEntity {
            id: 1,
            account_id: 1,
            name: "withdraw account".to_string(),
            address: "TWithdrawOwner".to_string(),
            pubkey: Some("pubkey".to_string()),
            address_type: "".to_string(),
            wallet_address: "TWithdrawWallet".to_string(),
            uid: "withdraw-uid".to_string(),
            derivation_path: "m/44'/195'/0'/0/0".to_string(),
            derivation_path_index: 0,
            chain_code: "tron".to_string(),
            api_wallet_type,
            status: 1,
            is_init: 1,
            is_expand: 0,
            is_used: true,
            created_at: chrono::Utc::now(),
            updated_at: None,
        }
    }
}
