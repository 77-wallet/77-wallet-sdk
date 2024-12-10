use crate::{
    api::ReturnType,
    request::transaction,
    response_vo::{
        self,
        multisig_account::{
            AddressStatus, MultisigAccountInfo, MultisigAccountList, MultisigFeeVo,
        },
    },
    service::multisig_account::MultisigAccountService,
};
use wallet_database::{entities::multisig_member::MemberVo, pagination::Pagination};
use wallet_transport_backend::DepositAddress;

impl crate::WalletManager {
    pub async fn create_multisig_account(
        &self,
        name: String,
        address: String,
        chain_code: String,
        threshold: i32,
        member_list: Vec<MemberVo>,
        address_type: Option<String>,
    ) -> ReturnType<()> {
        let service = MultisigAccountService::new(self.repo_factory.multisig_account_repo())?;
        service
            .crate_account(
                name,
                address,
                chain_code,
                threshold,
                member_list,
                address_type,
            )
            .await
            .into()
    }

    pub async fn multisig_account_by_id(
        &self,
        id: String,
    ) -> ReturnType<Option<MultisigAccountInfo>> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .multisig_account_by_id(&id)
            .await
            .into()
    }

    pub async fn multisig_account_by_address(
        &self,
        address: String,
    ) -> ReturnType<Option<MultisigAccountInfo>> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .multisig_account_by_address(&address)
            .await
            .into()
    }

    pub async fn multisig_account_lists(
        &self,
        owner: bool,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<MultisigAccountList>> {
        let service = MultisigAccountService::new(self.repo_factory.multisig_account_repo())?;
        service
            .account_list(owner, chain_code.as_deref(), page, page_size)
            .await
            .into()
    }

    pub async fn update_multisig_name(&self, account_id: String, name: String) -> ReturnType<()> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .update_multisig_name(account_id, name)
            .await
            .into()
    }

    // cancel account
    pub async fn cancel_multisig(&self, account_id: String) -> ReturnType<()> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .cancel_multisig(account_id)
            .await
            .into()
    }

    /// Deploys a new multisig account on the blockchain.
    pub async fn deploy_multisig_account(
        &self,
        account_id: String,
        deploy_fee: Option<String>,
        payer: Option<transaction::ServiceFeePayer>,
        password: String,
    ) -> ReturnType<()> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .deploy_multisig_account(&account_id, deploy_fee, payer, &password)
            .await
            .into()
    }

    pub async fn check_participant_exists(&self, account_id: String) -> ReturnType<Vec<String>> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .check_participant_exists(account_id)
            .await
            .into()
    }

    // confirm
    pub async fn confirm_participation(&self, account_id: String) -> ReturnType<()> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .confirm_participation(&account_id)
            .await
            .into()
    }

    /// Gets deploy multisig account fee.
    pub async fn get_account_fee(
        &self,
        account_id: String,
    ) -> ReturnType<response_vo::EstimateFeeResp> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .deploy_multisig_fee(&account_id)
            .await
            .into()
    }

    /// Gets the multisig service fee for the specified chain code.
    pub async fn get_multisig_service_fee(&self, chain_code: String) -> ReturnType<MultisigFeeVo> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .get_multisig_service_fee(&chain_code)
            .await
            .into()
    }

    /// Fetch the deposit address of the specified chain code.
    pub async fn fetch_deposit_address(&self, chain_code: String) -> ReturnType<DepositAddress> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .fetch_deposit_address(&chain_code)
            .await
            .into()
    }

    pub async fn whether_multisig_address(
        &self,
        address: String,
        chain_code: String,
    ) -> ReturnType<AddressStatus> {
        MultisigAccountService::new(self.repo_factory.multisig_account_repo())?
            .whether_multisig_address(address, chain_code)
            .await
            .into()
    }
}
