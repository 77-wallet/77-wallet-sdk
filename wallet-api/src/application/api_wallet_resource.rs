use crate::{
    application::wallet::WalletApplication,
    context::Context,
    domain::api_wallet::resource::ApiResourceDomain,
    error::service::ServiceError,
    messaging::notify::{
        FrontendNotifyEvent,
        event::NotifyEvent,
        other::{Process, TransactionProcessFrontend},
    },
    request::stake::{
        BatchDelegate, BatchUnDelegate, DelegateReq, FreezeBalanceReq, UnDelegateReq,
        UnFreezeBalanceReq, VoteWitnessReq, WithdrawBalanceReq,
    },
    response_vo::standard_wallet::stake::{
        BatchDelegateResp, DelegateResp, FreezeResp, VoteListResp, VoterInfoResp,
        WithdrawUnfreezeResp,
    },
    service::stake::StackService,
};
use wallet_chain_interact::tron::operations::stake::ResourceType;
use wallet_database::{
    entities::{
        api_resource_operation::{ApiResourceOperationType, NewApiResourceOperation},
        api_resource_type::ApiResourceType as DbApiResourceType,
        bill::BillKind,
    },
    repositories::api_wallet::resource_operation::ApiResourceOperationRepo,
};

pub(crate) struct ApiResourceApplication {
    ctx: &'static Context,
}

impl ApiResourceApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub(crate) async fn stake_withdraw_wallet_resource(
        &self,
        req: FreezeBalanceReq,
        password: String,
    ) -> Result<FreezeResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;

        let resource = ResourceType::try_from(req.resource.as_str())?;
        let bill_kind = match resource {
            ResourceType::BANDWIDTH => BillKind::FreezeBandwidth,
            ResourceType::ENERGY => BillKind::FreezeEnergy,
        };
        self.notify_transaction_process(bill_kind, Process::Building).await?;

        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let resource_trade_no = Self::client_resource_trade_no();
        self.create_client_resource_operation_pending(
            &owner_ctx.uid,
            &resource_trade_no,
            owner_ctx.owner_address,
            resource,
            req.frozen_balance.to_string(),
            ApiResourceOperationType::Stake,
        )
        .await?;

        let outcome = ApiResourceDomain::stake_withdraw_wallet_resource(self.ctx, &req).await?;

        self.complete_client_resource_operation_broadcast(
            &resource_trade_no,
            None,
            &outcome.tx_hash,
            &outcome.raw_tx,
            &outcome.transaction_fee,
        )
        .await?;

        outcome.resp.ok_or_else(|| {
            ServiceError::System(crate::error::system::SystemError::Internal(
                "api wallet stake response missing".to_string(),
            ))
        })
    }

    pub(crate) async fn unstake_withdraw_wallet_resource(
        &self,
        req: UnFreezeBalanceReq,
        password: String,
    ) -> Result<FreezeResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;

        let resource = ResourceType::try_from(req.resource.as_str())?;
        let bill_kind = match resource {
            ResourceType::BANDWIDTH => BillKind::UnFreezeBandwidth,
            ResourceType::ENERGY => BillKind::UnFreezeEnergy,
        };
        self.notify_transaction_process(bill_kind, Process::Building).await?;

        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let resource_trade_no = Self::client_resource_trade_no();
        self.create_client_resource_operation_pending(
            &owner_ctx.uid,
            &resource_trade_no,
            owner_ctx.owner_address,
            resource,
            req.unfreeze_balance.to_string(),
            ApiResourceOperationType::Unstake,
        )
        .await?;

        let outcome = ApiResourceDomain::unstake_withdraw_wallet_resource(self.ctx, &req).await?;

        self.complete_client_resource_operation_broadcast(
            &resource_trade_no,
            None,
            &outcome.tx_hash,
            &outcome.raw_tx,
            &outcome.transaction_fee,
        )
        .await?;

        outcome.resp.ok_or_else(|| {
            ServiceError::System(crate::error::system::SystemError::Internal(
                "api wallet unstake response missing".to_string(),
            ))
        })
    }

    pub(crate) async fn delegate_withdraw_wallet_resource(
        &self,
        req: DelegateReq,
        password: String,
    ) -> Result<DelegateResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;
        let resource = ResourceType::try_from(req.resource.as_str())?;
        let bill_kind = ApiResourceDomain::delegate_bill_kind(resource);
        self.notify_transaction_process(bill_kind, Process::Building).await?;
        let receiver_address = req.receiver_address.clone();
        let outcome = ApiResourceDomain::delegate_withdraw_wallet_resource(self.ctx, req).await?;
        self.record_client_resource_broadcast(
            outcome.uid.as_deref().ok_or_else(|| Self::missing_outcome("delegate uid"))?,
            &outcome.owner_address,
            receiver_address,
            resource,
            outcome.amount.clone().ok_or_else(|| Self::missing_outcome("delegate amount"))?,
            ApiResourceOperationType::Delegate,
            outcome.tx_hash.clone(),
            outcome.raw_tx.clone(),
            outcome.transaction_fee.clone(),
        )
        .await?;
        outcome.delegate_resp.ok_or_else(|| Self::missing_outcome("delegate response"))
    }

    pub(crate) async fn undelegate_withdraw_wallet_resource(
        &self,
        req: UnDelegateReq,
        password: String,
    ) -> Result<DelegateResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;
        let resource = ResourceType::try_from(req.resource.as_str())?;
        let bill_kind = ApiResourceDomain::undelegate_bill_kind(resource);
        self.notify_transaction_process(bill_kind, Process::Building).await?;
        let receiver_address = req.receiver_address.clone();
        let outcome = ApiResourceDomain::undelegate_withdraw_wallet_resource(self.ctx, req).await?;
        self.record_client_resource_broadcast(
            outcome.uid.as_deref().ok_or_else(|| Self::missing_outcome("undelegate uid"))?,
            &outcome.owner_address,
            receiver_address,
            resource,
            outcome.amount.clone().ok_or_else(|| Self::missing_outcome("undelegate amount"))?,
            ApiResourceOperationType::Undelegate,
            outcome.tx_hash.clone(),
            outcome.raw_tx.clone(),
            outcome.transaction_fee.clone(),
        )
        .await?;
        outcome.delegate_resp.ok_or_else(|| Self::missing_outcome("undelegate response"))
    }

    pub(crate) async fn batch_delegate_withdraw_wallet_resource(
        &self,
        req: BatchDelegate,
        password: String,
    ) -> Result<BatchDelegateResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;
        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let resource = ResourceType::try_from(req.resource_type.as_str())?;
        let outcome =
            ApiResourceDomain::batch_delegate_withdraw_wallet_resource(self.ctx, req).await?;
        self.record_client_batch_resource_broadcasts(
            &owner_ctx.uid,
            &owner_ctx.owner_address,
            resource,
            ApiResourceOperationType::BatchDelegate,
            outcome.broadcasts,
        )
        .await?;
        Ok(outcome.resp)
    }

    pub(crate) async fn batch_undelegate_withdraw_wallet_resource(
        &self,
        req: BatchUnDelegate,
        password: String,
    ) -> Result<BatchDelegateResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;
        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let resource = ResourceType::try_from(req.resource_type.as_str())?;
        let outcome =
            ApiResourceDomain::batch_undelegate_withdraw_wallet_resource(self.ctx, req).await?;
        self.record_client_batch_resource_broadcasts(
            &owner_ctx.uid,
            &owner_ctx.owner_address,
            resource,
            ApiResourceOperationType::BatchUndelegate,
            outcome.broadcasts,
        )
        .await?;
        Ok(outcome.resp)
    }

    pub(crate) async fn withdraw_wallet_votes(
        &self,
        req: VoteWitnessReq,
        password: &str,
    ) -> Result<String, ServiceError> {
        WalletApplication::validate_password(self.ctx, password).await?;
        self.notify_transaction_process(BillKind::Vote, Process::Building).await?;

        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let resource_trade_no = Self::client_resource_trade_no();
        self.create_client_resource_operation_pending(
            &owner_ctx.uid,
            &resource_trade_no,
            owner_ctx.owner_address,
            ResourceType::BANDWIDTH,
            req.get_votes().to_string(),
            ApiResourceOperationType::Vote,
        )
        .await?;

        let outcome = ApiResourceDomain::withdraw_wallet_votes(self.ctx, req).await?;
        self.complete_client_resource_operation_broadcast(
            &resource_trade_no,
            None,
            &outcome.tx_hash,
            &outcome.raw_tx,
            &outcome.transaction_fee,
        )
        .await?;
        Ok(outcome.tx_hash)
    }

    pub(crate) async fn withdraw_wallet_voter_info(
        &self,
        owner_address: &str,
    ) -> Result<VoterInfoResp, ServiceError> {
        ApiResourceDomain::withdraw_wallet_account_context(self.ctx, owner_address).await?;
        StackService::new(self.ctx).await?.voter_info(&owner_address).await
    }

    pub(crate) async fn withdraw_wallet_votes_node_list(
        &self,
        owner_address: Option<&str>,
    ) -> Result<VoteListResp, ServiceError> {
        if let Some(owner_address) = owner_address {
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, owner_address).await?;
        }
        StackService::new(self.ctx).await?.vote_list(owner_address).await
    }

    pub(crate) async fn withdraw_wallet_claim_votes_rewards(
        &self,
        req: WithdrawBalanceReq,
        password: &str,
    ) -> Result<String, ServiceError> {
        WalletApplication::validate_password(self.ctx, password).await?;
        self.notify_transaction_process(BillKind::WithdrawReward, Process::Building).await?;

        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let resource_trade_no = Self::client_resource_trade_no();
        self.create_client_resource_operation_pending(
            &owner_ctx.uid,
            &resource_trade_no,
            owner_ctx.owner_address,
            ResourceType::BANDWIDTH,
            "0".to_string(),
            ApiResourceOperationType::WithdrawReward,
        )
        .await?;

        let outcome = ApiResourceDomain::withdraw_wallet_claim_votes_rewards(self.ctx, req).await?;
        self.complete_client_resource_operation_broadcast(
            &resource_trade_no,
            outcome.amount.clone(),
            &outcome.tx_hash,
            &outcome.raw_tx,
            &outcome.transaction_fee,
        )
        .await?;
        Ok(outcome.tx_hash)
    }

    pub(crate) async fn withdraw_wallet_unfreeze(
        &self,
        req: WithdrawBalanceReq,
        password: String,
    ) -> Result<WithdrawUnfreezeResp, ServiceError> {
        WalletApplication::validate_password(self.ctx, &password).await?;
        self.notify_transaction_process(BillKind::WithdrawUnFreeze, Process::Building).await?;

        let owner_ctx =
            ApiResourceDomain::withdraw_wallet_account_context(self.ctx, &req.owner_address)
                .await?;
        let withdraw_amount_sun =
            ApiResourceDomain::withdraw_wallet_unfreeze_amount(self.ctx, &req.owner_address)
                .await?;
        let resource_trade_no = Self::client_resource_trade_no();
        self.create_client_resource_operation_pending(
            &owner_ctx.uid,
            &resource_trade_no,
            owner_ctx.owner_address,
            ResourceType::BANDWIDTH,
            withdraw_amount_sun.to_string(),
            ApiResourceOperationType::WithdrawUnfreeze,
        )
        .await?;

        let outcome =
            ApiResourceDomain::withdraw_wallet_unfreeze(self.ctx, req, withdraw_amount_sun).await?;
        self.complete_client_resource_operation_broadcast(
            &resource_trade_no,
            outcome.amount.clone(),
            &outcome.tx_hash,
            &outcome.raw_tx,
            &outcome.transaction_fee,
        )
        .await?;

        outcome.withdraw_unfreeze_resp.ok_or_else(|| {
            ServiceError::System(crate::error::system::SystemError::Internal(
                "api wallet withdraw unfreeze response missing".to_string(),
            ))
        })
    }

    async fn notify_transaction_process(
        &self,
        bill_kind: BillKind,
        process: Process,
    ) -> Result<(), ServiceError> {
        let data =
            NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(bill_kind, process));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_client_resource_operation_pending(
        &self,
        uid: &str,
        resource_trade_no: &str,
        owner_address: String,
        resource: ResourceType,
        amount: String,
        operation_type: ApiResourceOperationType,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        let input = NewApiResourceOperation::client(
            uid,
            resource_trade_no,
            owner_address,
            Self::db_resource_type(resource),
            amount,
            operation_type,
        );
        ApiResourceOperationRepo::upsert(&pool, input)
            .await
            .map_err(|e| ServiceError::Database(e.into()))
    }

    async fn complete_client_resource_operation_broadcast(
        &self,
        resource_trade_no: &str,
        amount: Option<String>,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        let existing = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, resource_trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
        let input = NewApiResourceOperation::client(
            existing.uid,
            existing.resource_trade_no,
            existing.owner_address,
            existing.resource_type,
            amount.unwrap_or(existing.amount),
            existing.operation_type,
        );
        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input,
            tx_hash,
            raw_tx,
            transaction_fee,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))
    }

    pub(crate) async fn record_client_batch_resource_broadcasts(
        &self,
        uid: &str,
        owner_address: &str,
        resource: ResourceType,
        operation_type: ApiResourceOperationType,
        broadcasts: Vec<crate::domain::api_wallet::resource::ApiBatchResourceBroadcast>,
    ) -> Result<(), ServiceError> {
        for broadcast in broadcasts {
            self.record_client_resource_broadcast(
                uid,
                owner_address,
                broadcast.receiver_address,
                resource,
                broadcast.amount.to_string(),
                operation_type,
                broadcast.tx_hash,
                broadcast.raw_tx,
                broadcast.transaction_fee,
            )
            .await?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn record_client_resource_broadcast(
        &self,
        uid: &str,
        owner_address: &str,
        receiver_address: String,
        resource: ResourceType,
        amount: String,
        operation_type: ApiResourceOperationType,
        tx_hash: String,
        raw_tx: String,
        transaction_fee: String,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        let mut input = NewApiResourceOperation::client(
            uid,
            Self::client_resource_trade_no(),
            owner_address,
            Self::db_resource_type(resource),
            amount,
            operation_type,
        );
        input.receiver_address = Some(receiver_address);
        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input,
            &tx_hash,
            &raw_tx,
            &transaction_fee,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))
    }

    fn missing_outcome(field: &str) -> ServiceError {
        ServiceError::System(crate::error::system::SystemError::Internal(format!(
            "api wallet resource outcome missing {field}"
        )))
    }

    fn db_resource_type(resource: ResourceType) -> DbApiResourceType {
        match resource {
            ResourceType::BANDWIDTH => DbApiResourceType::Bandwidth,
            ResourceType::ENERGY => DbApiResourceType::Energy,
        }
    }

    fn client_resource_trade_no() -> String {
        format!("client_rsc_{}", uuid::Uuid::new_v4().simple())
    }
}
