use super::ReturnType;
use crate::{
    request::stake::{DelegateReq, FreezeBalanceReq, UnFreezeBalanceReq},
    response_vo::account::AccountResource,
    service::stake::StackService,
};
use wallet_database::{
    entities::stake::{DelegateEntity, UnFreezeEntity},
    pagination::Pagination,
};
use wallet_transport_backend::response_vo::stake::SystemEnergyResp;

impl crate::WalletManager {
    // account resource
    pub async fn resource_info(&self, account: String) -> ReturnType<AccountResource> {
        let service = StackService::new(self.repo_factory.stake_repo());
        service.account_resource(&account).await?.into()
    }

    // freeze balance
    pub async fn freeze_balance(
        &self,
        req: FreezeBalanceReq,
        password: String,
    ) -> ReturnType<String> {
        StackService::new(self.repo_factory.stake_repo())
            .freeze_balance(req, &password)
            .await?
            .into()
    }

    // un freeze balance
    pub async fn un_freeze_balance(
        &self,
        req: UnFreezeBalanceReq,
        password: String,
    ) -> ReturnType<String> {
        StackService::new(self.repo_factory.stake_repo())
            .un_freeze_balance(req, &password)
            .await?
            .into()
    }

    // freeze list
    pub async fn freeze_list(
        &self,
        owner_address: String,
        resource_type: String,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<UnFreezeEntity>> {
        StackService::new(self.repo_factory.stake_repo())
            .freeze_list(&owner_address, &resource_type, page, page_size)
            .await?
            .into()
    }

    /// Withdraws any unfrozen balances for the given owner address.
    pub async fn withdraw_unfreeze(
        &self,
        owner_address: String,
        password: String,
    ) -> ReturnType<String> {
        StackService::new(self.repo_factory.stake_repo())
            .withdraw_unfreeze(&owner_address, &password)
            .await?
            .into()
    }

    pub async fn delegate_resource(
        &self,
        req: DelegateReq,
        password: String,
    ) -> ReturnType<String> {
        StackService::new(self.repo_factory.stake_repo())
            .delegate_resource(req, &password)
            .await?
            .into()
    }

    pub async fn delegate_list(
        &self,
        owner_address: String,
        resource_type: String,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<DelegateEntity>> {
        StackService::new(self.repo_factory.stake_repo())
            .delegate_list(&owner_address, &resource_type, page, page_size)
            .await?
            .into()
    }

    pub async fn un_delegate_resource(&self, id: String, password: String) -> ReturnType<String> {
        StackService::new(self.repo_factory.stake_repo())
            .un_delegate_resource(id, &password)
            .await?
            .into()
    }

    pub async fn request_resource(
        &self,
        _account: String,
        _energy: i64,
        _bandwidth: i64,
        _value: String,
        _symbol: String,
        _to: String,
    ) -> ReturnType<String> {
        // let repo = self.repo_factory.stake_repo();
        // StackService::new(repo)
        //     .request_resource(account, energy, bandwidth, &value, &symbol, &to)
        //     .await?
        //     .into()
        "used request_energy to instead".to_string().into()
    }

    pub async fn system_resource(&self, account: String) -> ReturnType<SystemEnergyResp> {
        StackService::new(self.repo_factory.stake_repo())
            .system_resource(account)
            .await?
            .into()
    }

    pub async fn request_energy(&self, account: String, energy: i64) -> ReturnType<String> {
        StackService::new(self.repo_factory.stake_repo())
            .request_energy(account, energy)
            .await?
            .into()
    }
}
