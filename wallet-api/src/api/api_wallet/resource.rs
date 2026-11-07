use crate::{
    api::ReturnType,
    manager::WalletManager,
    request::stake::{
        BatchDelegate, BatchUnDelegate, DelegateReq, FreezeBalanceReq, UnDelegateReq,
        UnFreezeBalanceReq, VoteWitnessReq, WithdrawBalanceReq,
    },
    response_vo::standard_wallet::stake::{
        BatchDelegateResp, DelegateResp, FreezeResp, VoteListResp, VoterInfoResp,
        WithdrawUnfreezeResp,
    },
    service::api_wallet::resource::ApiResourceService,
};

impl WalletManager {
    pub async fn stake_api_withdraw_wallet_resource(
        &self,
        req: FreezeBalanceReq,
        password: String,
    ) -> ReturnType<FreezeResp> {
        ApiResourceService::new(self.ctx).stake_withdraw_wallet_resource(req, password).await
    }

    pub async fn unstake_api_withdraw_wallet_resource(
        &self,
        req: UnFreezeBalanceReq,
        password: String,
    ) -> ReturnType<FreezeResp> {
        ApiResourceService::new(self.ctx).unstake_withdraw_wallet_resource(req, password).await
    }

    pub async fn delegate_api_withdraw_wallet_resource(
        &self,
        req: DelegateReq,
        password: String,
    ) -> ReturnType<DelegateResp> {
        ApiResourceService::new(self.ctx).delegate_withdraw_wallet_resource(req, password).await
    }

    pub async fn undelegate_api_withdraw_wallet_resource(
        &self,
        req: UnDelegateReq,
        password: String,
    ) -> ReturnType<DelegateResp> {
        ApiResourceService::new(self.ctx).undelegate_withdraw_wallet_resource(req, password).await
    }

    pub async fn batch_delegate_api_withdraw_wallet_resource(
        &self,
        req: BatchDelegate,
        password: String,
    ) -> ReturnType<BatchDelegateResp> {
        ApiResourceService::new(self.ctx)
            .batch_delegate_withdraw_wallet_resource(req, password)
            .await
    }

    pub async fn batch_undelegate_api_withdraw_wallet_resource(
        &self,
        req: BatchUnDelegate,
        password: String,
    ) -> ReturnType<BatchDelegateResp> {
        ApiResourceService::new(self.ctx)
            .batch_undelegate_withdraw_wallet_resource(req, password)
            .await
    }

    pub async fn api_withdraw_wallet_votes(
        &self,
        req: VoteWitnessReq,
        password: &str,
    ) -> ReturnType<String> {
        ApiResourceService::new(self.ctx).withdraw_wallet_votes(req, password).await
    }

    pub async fn api_withdraw_wallet_voter_info(
        &self,
        owner_address: &str,
    ) -> ReturnType<VoterInfoResp> {
        ApiResourceService::new(self.ctx).withdraw_wallet_voter_info(owner_address).await
    }

    pub async fn api_withdraw_wallet_votes_node_list(
        &self,
        owner_address: Option<&str>,
    ) -> ReturnType<VoteListResp> {
        ApiResourceService::new(self.ctx).withdraw_wallet_votes_node_list(owner_address).await
    }

    pub async fn api_withdraw_wallet_claim_votes_rewards(
        &self,
        req: WithdrawBalanceReq,
        password: &str,
    ) -> ReturnType<String> {
        ApiResourceService::new(self.ctx).withdraw_wallet_claim_votes_rewards(req, password).await
    }

    pub async fn api_withdraw_wallet_unfreeze(
        &self,
        req: WithdrawBalanceReq,
        password: String,
    ) -> ReturnType<WithdrawUnfreezeResp> {
        ApiResourceService::new(self.ctx).withdraw_wallet_unfreeze(req, password).await
    }
}
