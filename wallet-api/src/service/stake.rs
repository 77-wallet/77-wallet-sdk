use crate::domain;
use crate::domain::chain::adapter::TransactionAdapter;
use crate::error::business::stake::StakeError;
use crate::manager::Context;
use crate::request::stake;
use crate::response_vo::account::AccountResource;
use crate::BusinessError;
use wallet_chain_interact::factory::ChainFactory;
use wallet_chain_interact::tron::params as tron_params;
use wallet_chain_interact::tron::TronBlockChain;
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_database::entities::chain::ChainEntity;
use wallet_database::entities::stake::DelegateEntity;
use wallet_database::entities::stake::NewDelegateEntity;
use wallet_database::entities::stake::NewUnFreezeEntity;
use wallet_database::entities::stake::UnFreezeEntity;
use wallet_database::pagination::Pagination;
use wallet_database::repositories::stake as db_stake;
use wallet_transport_backend::response_vo::stake::SystemEnergyResp;

pub struct StackService {
    repo: db_stake::StakeRepo,
}

impl StackService {
    pub fn new(repo: db_stake::StakeRepo) -> Self {
        Self { repo }
    }

    async fn get_chain(&self) -> Result<TronBlockChain, crate::error::ServiceError> {
        let chain_code = "tron";
        let tx = Context::get_global_sqlite_pool()?;
        let node = ChainEntity::chain_node_info(tx.as_ref(), chain_code)
            .await
            .map_err(crate::SystemError::Database)?
            .ok_or(crate::BusinessError::Chain(crate::ChainError::NotFound(
                chain_code.to_string(),
            )))?;
        Ok(ChainFactory::tron_chain(&node.rpc_url)?)
    }

    async fn get_key(
        &self,
        address: &str,
        password: &str,
    ) -> Result<ChainPrivateKey, crate::error::ServiceError> {
        crate::domain::account::open_account_pk_with_password("tron", address, password).await
    }
}

impl StackService {
    pub async fn freeze_balance(
        &self,
        req: stake::FreezeBalanceReq,
        password: &str,
    ) -> Result<String, crate::error::ServiceError> {
        let tron_chain = self.get_chain().await?;
        let key = self.get_key(&req.owner_address, password).await?;

        let args = tron_params::FreezeBalanceArgs::try_from(req)?;
        let res = tron_chain.freeze_balance(args, key).await?;

        Ok(res)
    }

    pub async fn freeze_list(
        &self,
        owner: &str,
        resource_type: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<UnFreezeEntity>, crate::error::ServiceError> {
        Ok(self
            .repo
            .unfreeze_list(owner, resource_type, page, page_size)
            .await?)
    }

    pub async fn un_freeze_balance(
        &self,
        req: stake::UnFreezeBalanceReq,
        password: &str,
    ) -> Result<String, crate::error::ServiceError> {
        let tron_chain = self.get_chain().await?;

        let mut new_unfreeze = NewUnFreezeEntity::from(&req);

        let key = self.get_key(&req.owner_address, password).await?;
        let args = tron_params::UnFreezeBalanceArgs::try_from(req)?;

        let res = tron_chain.unfreeze_balance(args, key).await?;

        new_unfreeze.tx_hash = res.clone();
        new_unfreeze.freeze_time = wallet_utils::time::now_plus_days(14).timestamp();
        self.repo.add_unfreeze(new_unfreeze).await?;

        Ok(res)
    }

    pub async fn withdraw_unfreeze(
        &self,
        owner_address: &str,
        password: &str,
    ) -> Result<String, crate::error::ServiceError> {
        let tron_chain = self.get_chain().await?;
        let key = self.get_key(owner_address, password).await?;

        let res = tron_chain
            .withdraw_unfreeze_amount(owner_address, key)
            .await?;

        Ok(res)
    }

    pub async fn delegate_resource(
        &self,
        req: stake::DelegateReq,
        password: &str,
    ) -> Result<String, crate::error::ServiceError> {
        let tron_chain = self.get_chain().await?;
        let key = self.get_key(&req.owner_address, password).await?;

        let mut new_delegate = NewDelegateEntity::from(&req);

        let args = tron_params::DelegateArgs::try_from(req)?;
        let res = tron_chain.delegate_resource(args, key).await?;

        new_delegate.tx_hash = res.clone();
        self.repo.add_delegate(new_delegate).await?;

        Ok(res)
    }

    // Reclaim delegated energy
    pub async fn un_delegate_resource(
        &self,
        id: String,
        password: &str,
    ) -> Result<String, crate::error::ServiceError> {
        let tron_chain = self.get_chain().await?;

        let delegate = self.repo.find_delegate_by_id(&id).await?;
        let key = self.get_key(&delegate.owner_address, password).await?;

        let args = tron_params::UnDelegateArgs::new(
            &delegate.owner_address,
            &delegate.receiver_address,
            &delegate.amount.to_string(),
            &delegate.resource_type,
        )?;

        let res = tron_chain.un_delegate_resource(args, key).await?;

        // update delegate status
        self.repo.update_delegate(&id).await?;

        Ok(res)
    }

    pub async fn delegate_list(
        &self,
        owner_address: &str,
        resource_type: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<DelegateEntity>, crate::error::ServiceError> {
        Ok(self
            .repo
            .delegate_list(owner_address, resource_type, page, page_size)
            .await?)
    }

    pub async fn system_resource(
        &self,
        address: String,
    ) -> Result<SystemEnergyResp, crate::error::ServiceError> {
        let backhand = Context::get_global_backend_api()?;

        let req = serde_json::json!({
            "address": address
        });
        let res = backhand
            .post_request::<_, SystemEnergyResp>("delegate/resource/limit", req)
            .await?;

        Ok(res)
    }

    pub async fn request_energy(
        &self,
        account: String,
        energy: i64,
    ) -> Result<String, crate::error::ServiceError> {
        let backhand = Context::get_global_backend_api()?;

        // 验证后端的配置(是否开启了能量的补偿)
        if !backhand.delegate_is_open().await? {
            return Err(BusinessError::Stake(StakeError::SwitchClose))?;
        }

        let tron_chain = self.get_chain().await?;

        // request resource
        let res = backhand.delegate_order(&account, energy).await?;

        let duration = tokio::time::Duration::from_millis(500);

        let mut query_time = 0;
        let mut energy_status = 0; // 0 initial status  1:failed 2:success
        loop {
            if energy_status < 2 {
                if let Some(hash) = &res.energy_hash {
                    energy_status = match tron_chain.query_tx_res(hash).await {
                        Ok(Some(rs)) if rs.status == 2 => 2,
                        Ok(_) => 1,
                        Err(_) => 1,
                    };
                }
            }

            if query_time > 2 || (energy_status == 2) {
                break;
            };
            query_time += 1;

            tracing::warn!("sleep query result of delegate");
            tokio::time::sleep(duration).await;
        }

        Ok(res.order_id)
    }

    pub async fn account_resource(
        &self,
        account: &str,
    ) -> Result<AccountResource, crate::error::ServiceError> {
        // let tron_chain = self.get_chain().await?;

        let chain =
            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter("tron").await?;

        let chain = match chain {
            TransactionAdapter::Tron(chain) => chain,
            _ => {
                return Err(crate::BusinessError::Chain(
                    crate::ChainError::NotSupportChain,
                ))?;
            }
        };

        let resource = chain.account_resource(account).await?;
        let account = chain.account_info(account).await?;

        let mut res: AccountResource = AccountResource {
            balance: (account.balance / 1_000_000).to_string(),
            ..Default::default()
        };

        // net
        res.bandwidth.total_resource = resource.net_limit + resource.free_net_limit;
        res.bandwidth.limit_resource = res.bandwidth.total_resource - resource.free_net_used;

        // 给别人质押的
        res.bandwidth
            .set_delegate_freeze_amount(account.delegated_bandwidth);
        // 别人给我的
        res.bandwidth
            .set_acquire_freeze_amount(account.acquired_bandwidth);
        // 自己质押的
        res.bandwidth
            .set_owner_freeze_amount(account.frozen_v2_owner(""));
        // 可解冻
        res.bandwidth
            .set_can_unfreeze_amount(account.frozen_v2_owner(""));
        // 可提现
        res.bandwidth
            .set_can_withdraw_unfreeze_amount(account.can_withdraw_unfreeze_amount(""));
        // 总共
        res.bandwidth.total_freeze_amount = res.bandwidth.delegate_freeze_amount
            + res.bandwidth.owner_freeze_amount
            + res.bandwidth.acquire_freeze_amount;
        res.bandwidth.price = resource.net_price();

        // energy
        res.energy.total_resource = resource.energy_limit;
        res.energy.limit_resource = resource.energy_limit - resource.energy_used;

        // 给别人质押的
        res.energy
            .set_delegate_freeze_amount(account.account_resource.delegated_energy);
        // 别人给我的
        res.energy
            .set_acquire_freeze_amount(account.account_resource.acquired_energy);
        // 自己质押的
        res.energy
            .set_owner_freeze_amount(account.frozen_v2_owner("ENERGY"));
        // 可解冻
        res.energy
            .set_can_unfreeze_amount(account.frozen_v2_owner("ENERGY"));
        // 可提现
        res.energy
            .set_can_withdraw_unfreeze_amount(account.can_withdraw_unfreeze_amount("ENERGY"));
        // 总共
        res.energy.total_freeze_amount = res.energy.delegate_freeze_amount
            + res.energy.owner_freeze_amount
            + res.energy.acquire_freeze_amount;
        res.energy.price = resource.energy_price();

        Ok(res)
    }
}
