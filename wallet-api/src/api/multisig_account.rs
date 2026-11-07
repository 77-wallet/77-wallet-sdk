use crate::{
    api::ReturnType,
    manager::WalletManager,
    request::transaction,
    response_vo::{
        self,
        standard_wallet::multisig_account::{
            AddressStatus, MultisigAccountInfo, MultisigAccountList, MultisigFeeVo,
        },
    },
    service::multisig_account::MultisigAccountService,
};
use wallet_database::{entities::multisig_member::MemberVo, pagination::Pagination};

impl WalletManager {
    pub async fn create_multisig_account(
        &self,
        name: String,
        address: String,
        chain_code: String,
        threshold: i32,
        member_list: Vec<MemberVo>,
        address_type: Option<String>,
    ) -> ReturnType<()> {
        // tracing::warn!("接收到前端参数{:?}", member_list);

        let service = MultisigAccountService::new(self.ctx)?;
        service.crate_account(name, address, chain_code, threshold, member_list, address_type).await
    }

    pub async fn multisig_account_by_id(
        &self,
        id: String,
    ) -> ReturnType<Option<MultisigAccountInfo>> {
        MultisigAccountService::new(self.ctx)?.multisig_account_by_id(&id).await
    }

    pub async fn multisig_account_by_address(
        &self,
        address: String,
    ) -> ReturnType<Option<MultisigAccountInfo>> {
        MultisigAccountService::new(self.ctx)?.multisig_account_by_address(&address).await
    }

    pub async fn multisig_account_lists(
        &self,
        owner: bool,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<MultisigAccountList>> {
        MultisigAccountService::new(self.ctx)?
            .account_list(owner, chain_code.as_deref(), page, page_size)
            .await
    }

    pub async fn update_multisig_name(&self, account_id: String, name: String) -> ReturnType<()> {
        MultisigAccountService::new(self.ctx)?.update_multisig_name(account_id, name).await
    }

    // cancel account
    pub async fn cancel_multisig(&self, account_id: String) -> ReturnType<()> {
        MultisigAccountService::new(self.ctx)?.cancel_multisig(account_id).await
    }

    /// Deploys a new multisig account on the blockchain.
    ///
    /// TODO: ServiceFeePayer 加了一个参数tokenAddress
    pub async fn deploy_multisig_account(
        &self,
        account_id: String,
        deploy_fee: Option<String>,
        payer: Option<transaction::ServiceFeePayer>,
        password: String,
    ) -> ReturnType<()> {
        MultisigAccountService::new(self.ctx)?
            .deploy_multisig_account(&account_id, deploy_fee, payer, &password)
            .await
    }

    pub async fn check_participant_exists(&self, account_id: String) -> ReturnType<Vec<String>> {
        MultisigAccountService::new(self.ctx)?.check_participant_exists(account_id).await
    }

    // confirm
    pub async fn confirm_participation(&self, account_id: String) -> ReturnType<()> {
        MultisigAccountService::new(self.ctx)?.confirm_participation(&account_id).await
    }

    /// Gets deploy multisig account fee.
    pub async fn get_account_fee(
        &self,
        account_id: String,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        MultisigAccountService::new(self.ctx)?.deploy_multisig_fee(&account_id).await
    }

    /// Gets the multisig service fee for the specified chain code.
    pub async fn get_multisig_service_fee(
        &self,
        pay_chain: String,
        account_chain: String,
        pay_address: String,
    ) -> ReturnType<MultisigFeeVo> {
        MultisigAccountService::new(self.ctx)?
            .get_multisig_service_fee(&pay_chain, &account_chain, &pay_address)
            .await
    }

    /// Fetch the deposit address of the specified chain code.
    pub async fn fetch_deposit_address(&self, chain_code: String) -> ReturnType<String> {
        MultisigAccountService::new(self.ctx)?.fetch_deposit_address(&chain_code).await
    }

    pub async fn whether_multisig_address(
        &self,
        address: String,
        chain_code: String,
    ) -> ReturnType<AddressStatus> {
        MultisigAccountService::new(self.ctx)?.whether_multisig_address(address, chain_code).await
    }
}
