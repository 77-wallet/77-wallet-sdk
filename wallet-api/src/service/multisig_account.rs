use crate::{
    domain::{
        self,
        assets::AssetsDomain,
        chain::{adapter::ChainAdapterFactory, transaction::ChainTransDomain},
        coin::CoinDomain,
        multisig::MultisigDomain,
        task_queue::TaskQueueDomain,
    },
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    messaging::mqtt::topics::OrderMultiSignAccept,
    request::transaction,
    response_vo::{
        self,
        standard_wallet::multisig_account::{
            AddressStatus, MultisigAccountInfo, MultisigAccountList, MultisigFeeVo,
        },
    },
};
use std::{collections::HashMap, sync::Arc};
use wallet_database::{
    CoreDbPool,
    entities::{
        bill::BillKind,
        coin::CoinMultisigStatus,
        multisig_account::{
            MultiAccountOwner, MultisigAccountEntity, MultisigAccountPayStatus,
            MultisigAccountStatus, NewMultisigAccountEntity,
        },
        multisig_member::{MemberVo, MultisigMemberEntities},
    },
    pagination::Pagination,
    repositories::{
        account::AccountRepo, assets::AssetsRepo, bill::BillRepo, chain::ChainRepo,
        multisig_member::MultisigMemberRepo, wallet::WalletRepo,
    },
};
use wallet_transport_backend::{
    AddressList, ConfirmedAddress, OrderMultisigUpdateArg, SignedOrderAcceptReq,
    SignedSaveAddressReq, SignedUpdateRechargeHashReq, SignedUpdateSignedHashReq,
    SingedOrderCancelReq,
    api::BackendApi,
    consts::endpoint,
    request::{SignedFeeListReq, SignedFindAddressReq},
};
use wallet_types::{
    chain::address::{category::BtcAddressCategory, r#type::BtcAddressType},
    constant::chain_code,
};
use wallet_utils::serde_func;

fn needs_tron_multisig_account_inspection(chain_code: &str) -> bool {
    chain_code == chain_code::TRON
}

fn is_evm_safe_chain(chain_code: &str) -> bool {
    matches!(
        chain_code,
        chain_code::ETHEREUM
            | chain_code::BNB
            | chain_code::POLYGON
            | chain_code::AVALANCHE
            | chain_code::ARBITRUM
            | chain_code::BASE
            | chain_code::SCROLL
    )
}

#[cfg(test)]
mod evm_multisig_address_status_tests {
    use super::needs_tron_multisig_account_inspection;

    #[test]
    fn polygon_status_does_not_require_a_transaction_adapter() {
        assert!(!needs_tron_multisig_account_inspection("polygon"));
    }

    #[test]
    fn tron_status_still_requires_on_chain_account_inspection() {
        assert!(needs_tron_multisig_account_inspection("tron"));
    }
}

pub struct MultisigAccountService {
    ctx: &'static crate::context::Context,
    backend: Arc<BackendApi>,
}

impl MultisigAccountService {
    pub fn new(
        ctx: &'static crate::context::Context,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api().clone();
        Ok(Self { ctx, backend })
    }

    pub async fn crate_account(
        self,
        name: String,
        address: String,
        chain_code: String,
        threshold: i32,
        mut member_list: Vec<MemberVo>,
        address_type: Option<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        // check address type
        let address_type = match chain_code.as_str() {
            chain_code::BTC => {
                let address_type =
                    address_type.ok_or(crate::error::service::ServiceError::Business(
                        crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::BitcoinAddressEmpty,
                        ),
                    ))?;
                let category = BtcAddressCategory::try_from(address_type)?;
                BtcAddressType::from(category).to_string()
            }
            _ => String::new(),
        };

        // check tron address repeat
        let account = wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_conditions(
            &CoreDbPool::new(pool.clone()),
            vec![("address", &address), ("is_del", "0")],
        )
        .await?;
        if let Some(account) = account
            && account.chain_code == chain_code::TRON
        {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::AddressRepeat,
                ),
            ))?;
        }

        let member_address = member_list.iter().map(|m| m.address.clone()).collect::<Vec<String>>();

        // 获取address对应的uid
        let address_uid = self.backend.get_address_uid(chain_code.clone(), member_address).await?;
        // 设置member 的ui
        for item in member_list.iter_mut() {
            if item.uid.is_empty() {
                match address_uid.list.iter().find(|uid| item.address == uid.address) {
                    Some(find) if !find.uid.is_empty() => {
                        item.uid = find.uid.clone();
                    }
                    _ => {
                        return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::MultisigAccount(
                            crate::error::business::multisig_account::MultisigAccountError::NotPlatFormAddress,
                        )))?;
                    }
                }
            }
        }

        let uid_list = WalletRepo::uid_list(CoreDbPool::new(pool.clone()))
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect();

        let mut params =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::build_new_account(
                None,
                name,
                address.clone(),
                address.clone(),
                chain_code.clone(),
                threshold,
                address_type,
                member_list,
                &uid_list,
            );

        self.multisig_account_name(&mut params).await?;
        self.mark_self_account(&mut params).await?;

        let raw_data = params.to_multisig_account_data().to_string()?;
        let sync_account_params = OrderMultiSignAccept::from(&params);

        let address_list = sync_account_params
            .member
            .iter()
            .map(|a| AddressList {
                name: a.name.clone(),
                address: a.address.clone(),
                pubkey: a.pubkey.clone(),
                confirmed: a.confirmed,
                uid: a.uid.clone(),
            })
            .collect::<Vec<AddressList>>();

        let req = SignedSaveAddressReq::new(
            &params.id,
            &params.chain_code,
            &params.initiator_addr,
            address_list,
            &sync_account_params.to_json_str()?,
            raw_data,
        );
        self.backend.signed_order_save_confirm_address(req).await?;

        wallet_database::repositories::multisig_account::MultisigAccountRepo::create_with_member(
            &CoreDbPool::new(pool.clone()),
            &params,
        )
        .await?;

        Ok(())
    }

    async fn multisig_account_name(
        &self,
        params: &mut NewMultisigAccountEntity,
    ) -> Result<(), crate::error::service::ServiceError> {
        if !params.name.is_empty() {
            return Ok(());
        }

        let core_pool = self.ctx.core_pool()?;
        let count =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::account_count(
                &core_pool,
                &params.chain_code,
            )
            .await;
        params.name = format!("Multisig-{}-{}", params.chain_code, count + 1);

        Ok(())
    }

    async fn mark_self_account(
        &self,
        params: &mut NewMultisigAccountEntity,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut flag = true;
        let core_pool = self.ctx.core_pool()?;

        for item in params.member_list.iter_mut() {
            if let Some(account) = wallet_database::repositories::multisig_account::MultisigAccountRepo::wallet_account(
                &core_pool,
                &item.address,
                &params.chain_code,
            )
            .await?
            {
                item.confirmed = 1;
                item.pubkey = account.pubkey.clone();

                if account.address != params.initiator_addr {
                    params.owner = MultiAccountOwner::Both;
                }
            }

            // 检查是否完全是自己的成员
            if item.is_self != 1 {
                flag = false;
            }
        }

        // 如果所有成员都是自己
        if flag {
            params.status = MultisigAccountStatus::Confirmed;
        }

        Ok(())
    }

    pub async fn multisig_account_by_id(
        self,
        id: &str,
    ) -> Result<Option<MultisigAccountInfo>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let account =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_id(
                &core_pool, id,
            )
            .await?;

        let mut account = match account {
            Some(account) => account,
            None => return Ok(None),
        };

        account.address_type_to_category();
        let member = MultisigMemberRepo::list_by_account_id(&core_pool, &account.id).await?.0;

        Ok(Some(MultisigAccountInfo { account, member }))
    }

    pub async fn multisig_account_by_address(
        &self,
        address: &str,
    ) -> Result<Option<MultisigAccountInfo>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let account = wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_condition(
            &core_pool,
            "address",
            address,
        )
        .await?;

        let mut account = match account {
            Some(account) => account,
            None => return Ok(None),
        };

        account.address_type_to_category();
        let member = MultisigMemberRepo::list_by_account_id(&core_pool, &account.id).await?.0;
        Ok(Some(MultisigAccountInfo { account, member }))
    }

    pub async fn update_multisig_name(
        &self,
        account_id: String,
        name: String,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        Ok(wallet_database::repositories::multisig_account::MultisigAccountRepo::update_name(
            &core_pool,
            &account_id,
            &name,
        )
        .await?)
    }

    pub async fn cancel_multisig(
        self,
        id: String,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let account =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_id(
                &core_pool, &id,
            )
            .await?
            .ok_or(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::NotFound,
                ),
            ))?;

        if account.status == MultisigAccountStatus::OnChain.to_i8() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::CannotCancel,
                ),
            ))?;
        }

        wallet_database::repositories::multisig_account::MultisigAccountRepo::cancel_multisig(
            &core_pool, &account,
        )
        .await?;

        // 上报后端
        let raw_data =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::multisig_data(
                &core_pool,
                &account.id,
            )
            .await?
            .to_string()?;
        let req = SingedOrderCancelReq { order_id: account.id.clone(), raw_data };
        TaskQueueDomain::send_or_to_queue_with_ctx(
            self.ctx,
            req,
            endpoint::multisig::SIGNED_ORDER_CANCEL,
        )
        .await?;

        Ok(())
    }

    pub async fn account_list(
        &self,
        owner: bool,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigAccountList>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        // 同步部署中多签账号的状态
        let _r = domain::multisig::MultisigDomain::sync_multisig_status(
            self.ctx,
            pool.clone().into_inner(),
        )
        .await;

        let mut res =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::account_list(
                &pool, owner, chain_code, page, page_size,
            )
            .await?;

        let mut list = vec![];
        // main symbol
        for item in res.data.iter_mut() {
            let chain = ChainRepo::detail(&pool, &item.chain_code).await?;
            if let Some(chain) = chain {
                item.address_type_to_category();
                list.push({
                    MultisigAccountList { account: item.clone(), symbol: chain.main_symbol }
                });
            }
        }

        let resp = Pagination {
            data: list,
            total_count: res.total_count,
            page: res.page,
            page_size: res.page_size,
        };

        Ok(resp)
    }

    pub async fn fetch_deposit_address(
        self,
        chain_code: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let req = SignedFindAddressReq::new(chain_code);
        let res = self
            .backend
            .signed_find_address(req)
            .await
            .map_err(crate::error::service::ServiceError::TransportBackend)?;

        Ok(res.address)
    }

    pub async fn get_multisig_service_fee(
        self,
        pay_chain: &str,
        account_chain: &str,
        pay_address: &str,
    ) -> Result<MultisigFeeVo, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let account_with_wallet =
            AccountRepo::account_with_wallet(pay_address, pay_chain, core_pool).await?;

        // service fee
        let req =
            SignedFeeListReq::new(account_chain, pay_chain, pay_address, account_with_wallet.uid);
        let res = self.backend.signed_fee_info_v3(req).await?;

        Ok(MultisigFeeVo::from(res))
    }

    pub async fn check_participant_exists(
        &self,
        account_id: String,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let multisig_account =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_id(
                &core_pool,
                &account_id,
            )
            .await?
            .ok_or(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::NotFound,
                ),
            ))?;

        // only my address
        let member = MultisigMemberRepo::get_self_by_id(&core_pool, &account_id).await?;

        let mut not_exits = vec![];

        for m in member.0 {
            let account = wallet_database::repositories::multisig_account::MultisigAccountRepo::wallet_account(
                &core_pool,
                &m.address,
                &multisig_account.chain_code,
            )
            .await?;
            if account.is_none() {
                not_exits.push(m.address);
            }
        }

        Ok(not_exits)
    }

    pub async fn confirm_participation(
        self,
        id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let multisig_account =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_id(
                &core_pool, id,
            )
            .await?
            .ok_or(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::NotFound,
                ),
            ))?;

        if multisig_account.is_del == 1 {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::IsCancel,
                ),
            ))?;
        }

        // only my address
        let mut self_address = MultisigMemberRepo::get_self_by_id(&core_pool, id).await?;
        // do update confirm status
        wallet_database::repositories::multisig_account::MultisigAccountRepo::update_confirm_status(
            &core_pool,
            &multisig_account.id,
            &multisig_account.chain_code,
            &mut self_address,
        )
        .await?;

        // upload backend
        let accept_address = self_address
            .0
            .iter()
            .map(|i| ConfirmedAddress {
                address: i.address.to_string(),
                pubkey: i.pubkey.clone(),
                status: 1,
                uid: i.uid.clone(),
            })
            .collect::<Vec<ConfirmedAddress>>();

        let raw_data =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::multisig_data(
                &core_pool, id,
            )
            .await?
            .to_string()?;
        let req = SignedOrderAcceptReq {
            order_id: multisig_account.id,
            accept_address,
            status: 1,
            raw_data,
        };
        TaskQueueDomain::send_or_to_queue_with_ctx(
            self.ctx,
            req,
            endpoint::multisig::SIGNED_ORDER_ACCEPT,
        )
        .await?;

        // let task = Task::BackendApi(BackendApiTask::BackendApi(BackendApiTaskData {
        //     endpoint: endpoint::multisig::SIGNED_ORDER_ACCEPT.to_string(),
        //     body: serde_func::serde_to_value(&req)?,
        // }));
        // Tasks::new().push(task).send().await?;

        Ok(())
    }

    pub async fn deploy_multisig_account(
        self,
        account_id: &str,
        deploy_fee: Option<String>,
        payer: Option<transaction::ServiceFeePayer>,
        password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let multisig_account =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::find_by_id(
                &core_pool, account_id,
            )
            .await?
            .ok_or(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::MultisigAccount(
                    crate::error::business::multisig_account::MultisigAccountError::NotFound,
                ),
            ))?;

        // Reject a misconfigured Safe RPC before charging the service fee.
        if matches!(
            multisig_account.chain_code.as_str(),
            chain_code::POLYGON
                | chain_code::AVALANCHE
                | chain_code::ARBITRUM
                | chain_code::BASE
                | chain_code::SCROLL
        ) {
            let adapter = ChainAdapterFactory::get_multisig_adapter_with_ctx(
                &self.ctx,
                &multisig_account.chain_code,
            )
            .await?;
            adapter.preflight_safe_deployment().await?;
        }

        let time = wallet_utils::time::now_utc_format_time();
        // service fee
        if multisig_account.pay_status != MultisigAccountPayStatus::Paid.to_i8() {
            if let Some(payer) = payer {
                let fee_chain = payer.chain_code.clone();
                let fee_res =
                    self._transfer_service_fee(&multisig_account, payer, password).await?;

                // 服务费完成后更新本地数据
                let params = HashMap::from([
                    ("fee_hash".to_string(), fee_res.0),
                    ("fee_chain".to_string(), fee_chain),
                    ("pay_status".to_string(), fee_res.1),
                    ("updated_at".to_string(), time.clone()),
                ]);
                let _ =
                    wallet_database::repositories::multisig_account::MultisigAccountRepo::update_by_id(
                        &core_pool, account_id, params,
                    )
                    .await?;
            } else {
                return Err(crate::error::service::ServiceError::Business(
                    crate::error::business::BusinessError::MultisigAccount(
                        crate::error::business::multisig_account::MultisigAccountError::PayerNeed,
                    ),
                ))?;
            };
        }

        // deploy account
        if multisig_account.status != MultisigAccountStatus::OnChain.to_i8() {
            // 波场的上报原始的权限
            if multisig_account.chain_code == chain_code::TRON {
                // 从脸上获取当前账号的情况
                let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(&self.ctx).await?;
                let account = chain.account_info(&multisig_account.address).await?;

                let users = account.all_actives_user();
                if users.len() > 1 || (users.len() == 1 && users[0] != multisig_account.address) {
                    let _n = self.backend.permission_clean(&multisig_account.address, users).await;
                }
            }

            let member =
                MultisigMemberRepo::list_by_account_id(&core_pool, &multisig_account.id).await?;

            let multisig_adapter = ChainAdapterFactory::get_multisig_adapter_with_ctx(
                &self.ctx,
                &multisig_account.chain_code,
            )
            .await?;

            // 有交易hash验证是否成功，如果已经成功了不在重复部署
            if !multisig_account.deploy_hash.is_empty() {
                let tx = multisig_adapter.query_tx_res(&multisig_account.deploy_hash).await?;
                if let Some(tx) = tx {
                    if tx.status != 3 {
                        return Ok(());
                    }
                } else if is_evm_safe_chain(&multisig_account.chain_code) {
                    // An unknown receipt is not proof that the deployment failed.
                    return Ok(());
                }
            }

            // if chain_code is bitcoin the status is success on chain
            let status = if multisig_account.chain_code == chain_code::BTC {
                MultisigAccountStatus::OnChain.to_i8().to_string()
            } else {
                MultisigAccountStatus::OnChianPending.to_i8().to_string()
            };

            let multisig_account = if is_evm_safe_chain(&multisig_account.chain_code)
                && !multisig_account.address.is_empty()
                && !multisig_account.salt.is_empty()
            {
                // A retry must deploy the Safe whose address was already persisted.
                multisig_account.clone()
            } else {
                let resp = multisig_adapter.multisig_address(&multisig_account, &member).await?;
                let hash_map = HashMap::from([
                    ("address".to_string(), resp.multisig_address.clone()),
                    ("salt".to_string(), resp.salt.clone()),
                    ("authority_addr".to_string(), resp.authority_address.clone()),
                    ("updated_at".to_string(), time.clone()),
                ]);
                let multisig_account = wallet_database::repositories::multisig_account::MultisigAccountRepo::update_by_id(
                    &core_pool,
                    &multisig_account.id,
                    hash_map,
                )
                .await?;

                multisig_account
            };

            // 地址保存后若资产初始化曾中断，重试仍需补齐默认资产。
            AssetsDomain::init_default_multisig_assets(
                self.ctx,
                multisig_account.address.clone(),
                multisig_account.chain_code.clone(),
            )
            .await?;
            if multisig_account.chain_code.as_str() == chain_code::TRON {
                let pool = self.ctx.core_pool()?;
                AssetsRepo::update_tron_multisig_assets(
                    &pool,
                    &multisig_account.address,
                    &multisig_account.chain_code,
                    CoinMultisigStatus::IsMultisig.to_i8(),
                )
                .await?;
            }

            // 部署多签账户到链上
            let hash = self
                ._deploy_account(multisig_account, &member, deploy_fee, password, &multisig_adapter)
                .await?;

            let params = HashMap::from([
                ("deploy_hash".to_string(), hash),
                ("status".to_string(), status),
                ("updated_at".to_string(), time.clone()),
            ]);
            let _ =
                wallet_database::repositories::multisig_account::MultisigAccountRepo::update_by_id(
                    &core_pool, account_id, params,
                )
                .await?;
        }

        // 最后更新raw_data
        let raw_data =
            wallet_database::repositories::multisig_account::MultisigAccountRepo::multisig_data(
                &core_pool,
                &multisig_account.id,
            )
            .await?;
        let body = serde_json::json!({
            "businessId":&multisig_account.id.clone(),
            "rawData":raw_data.to_string()?
        });
        let task = BackendApiTask::BackendApi(BackendApiTaskData {
            endpoint: endpoint::multisig::SIGNED_ORDER_SAVE_RAW_DATA.to_string(),
            body,
        });
        Tasks::new().push(task).send_with_ctx(self.ctx).await?;

        Ok(())
    }

    async fn _transfer_service_fee(
        &self,
        multisig_account: &MultisigAccountEntity,
        payer: transaction::ServiceFeePayer,
        password: &str,
    ) -> Result<(String, String), crate::error::service::ServiceError> {
        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &payer.chain_code)
                .await?;

        // 如果交易hash存在，验证交易是否成功了避免重复
        if !multisig_account.fee_hash.is_empty() {
            let tx = adapter.query_tx_res(&multisig_account.fee_hash).await?;
            if let Some(tx) = tx {
                if tx.status != 3 {
                    // 交易成功或者在确认中不在进行交易，只有失败才再次转账
                    let pay_status = if tx.status == 2 {
                        MultisigAccountPayStatus::Paid
                    } else {
                        MultisigAccountPayStatus::PaidPending
                    };
                    return Ok((multisig_account.fee_hash.clone(), pay_status.to_i8().to_string()));
                }
            }
        }

        let backend = self.ctx.get_global_backend_api();

        // fetch address
        let req = SignedFindAddressReq::new(&payer.chain_code);
        let address = backend.signed_find_address(req).await?;
        let to = &address.address;

        // fetch value
        let core_pool = self.ctx.core_pool()?;
        let account_with_wallet =
            AccountRepo::account_with_wallet(&payer.from, &payer.chain_code, core_pool).await?;

        let req = SignedFeeListReq::new(
            &multisig_account.chain_code,
            &payer.chain_code,
            &payer.from,
            account_with_wallet.uid,
        );
        let amount = backend.signed_fee_info_v3(req).await?;

        let tx_hash = if amount.free != 0.0 {
            let value = amount.free.to_string();
            let coin = CoinDomain::get_coin_by_token_key_with_ctx(
                self.ctx,
                &payer.chain_code,
                payer.token_key(),
            )
            .await?;
            // transfer parameter
            let mut base = transaction::BaseTransferReq::new(
                &payer.from,
                to,
                &value,
                &payer.chain_code,
                &payer.symbol,
            );
            base.with_token(coin.token_address.clone());
            base.with_decimals(coin.decimals);
            let params = transaction::TransferReq {
                base,
                password: password.to_string(),
                fee_setting: payer.fee_setting.unwrap_or_default(),
                signer: None,
            };

            let private_key = ChainTransDomain::get_key(
                self.ctx,
                &params.base.from,
                &params.base.chain_code,
                &params.password,
                &params.signer,
            )
            .await?;

            ChainTransDomain::transfer(
                self.ctx,
                params,
                BillKind::ServiceCharge,
                &adapter,
                private_key,
            )
            .await?
        } else {
            // 服务费为0的传入固定的hash
            MultisigAccountEntity::NONE_TRANS_HASH.to_string()
        };

        // sync to backend
        // let mut raw_data = MultisigAccountRepo::multisig_data(&core_pool, &multisig_account.id).await?;
        // raw_data.account.fee_chain = payer.chain_code.clone();
        // raw_data.account.updated_at = Some(wallet_utils::time::now());
        // raw_data.account.fee_hash = tx_hash.clone();
        // 改版后最后统一上报 raw_data
        let req = SignedUpdateRechargeHashReq {
            order_id: multisig_account.id.to_string(),
            hash: tx_hash.clone(),
            product_code: multisig_account.chain_code.clone(),
            receive_chain_code: payer.chain_code,
            receive_address: to.to_string(),
            raw_data: "".to_string(),
            score_trans_id: amount.score_trans_id,
        };
        let task = BackendApiTask::BackendApi(BackendApiTaskData {
            endpoint: endpoint::multisig::SIGNED_ORDER_UPDATE_RECHARGE_HASH.to_string(),
            body: serde_func::serde_to_value(&req)?,
        });
        Tasks::new().push(task).send_with_ctx(self.ctx).await?;

        let status = if amount.free != 0.0 {
            MultisigAccountPayStatus::PaidPending
        } else {
            MultisigAccountPayStatus::Paid
        };

        Ok((tx_hash, status.to_i8().to_string()))
    }

    async fn _deploy_account(
        &self,
        account: MultisigAccountEntity,
        members: &MultisigMemberEntities,
        deploy_fee: Option<String>,
        password: &str,
        adapter: &domain::chain::adapter::MultisigAdapter,
    ) -> Result<String, crate::error::service::ServiceError> {
        // 1.执行链上部署交易
        let key = domain::account::open_subpk_with_password(
            self.ctx,
            &account.chain_code,
            &account.initiator_addr,
            password,
        )
        .await?;

        let (hash, bill_consumer) =
            adapter.deploy_multisig_account(&account, members, deploy_fee, key).await?;

        // 2. 不是btc的创建一个部署的bill
        if account.chain_code != chain_code::BTC {
            let main_coin = domain::chain::transaction::ChainTransDomain::main_coin(
                self.ctx,
                &account.chain_code,
            )
            .await?;
            let mut new_bill = BillRepo::build_deploy_bill(
                hash.clone(),
                account.initiator_addr.clone(),
                main_coin.chain_code,
                main_coin.symbol,
            );
            new_bill.resource_consume = bill_consumer;
            let pool = self.ctx.core_pool()?;
            crate::domain::bill::BillDomain::create_bill(&pool, new_bill).await?;
        }

        // 3.同步后端数据(used to sync other member update data)
        let multisig_args = OrderMultisigUpdateArg {
            multisig_account_id: account.id.clone(),
            multisig_account_address: account.address.clone(),
            address_type: account.address_type.clone(),
            salt: account.salt.clone(),
            authority_addr: account.authority_addr.clone(),
        };

        // let mut raw_data = MultisigAccountRepo::multisig_data(&core_pool, &account.id).await?;
        // raw_data.account.deploy_hash = hash.clone();
        // raw_data.account.status = MultisigAccountStatus::OnChianPending.to_i8();
        let req = SignedUpdateSignedHashReq::new(
            &account.id,
            &hash,
            &account.address,
            &account.salt,
            &account.authority_addr,
            multisig_args.to_json_str()?,
        );
        let task = BackendApiTask::BackendApi(BackendApiTaskData {
            endpoint: endpoint::multisig::SIGNED_ORDER_UPDATE_SIGNED_HASH.to_string(),
            body: serde_func::serde_to_value(&req)?,
        });
        Tasks::new().push(task).send_with_ctx(self.ctx).await?;

        Ok(hash)
    }

    pub async fn deploy_multisig_fee(
        &self,
        account_id: &str,
    ) -> Result<response_vo::EstimateFeeResp, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;

        let account = MultisigDomain::account_by_id(account_id, pool.clone()).await?;

        let main_coin =
            domain::chain::transaction::ChainTransDomain::main_coin(self.ctx, &account.chain_code)
                .await?;

        let adapter =
            ChainAdapterFactory::get_multisig_adapter_with_ctx(&self.ctx, &account.chain_code)
                .await?;

        let core_pool = self.ctx.core_pool()?;
        let member = MultisigMemberRepo::list_by_account_id(&core_pool, account_id).await?;

        let backend = self.ctx.get_global_backend_api();
        let fee = adapter
            .deploy_multisig_fee_with_ctx(
                self.ctx,
                &account,
                member,
                &main_coin.symbol,
                backend.as_ref(),
            )
            .await?;

        let fee_resp =
            response_vo::EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code, fee);
        Ok(fee_resp)
    }

    // 地址的状态:0,默认  1 已经是多签账号了  2 这个地址作为init_address 存在还未部署完成的多签账号
    pub async fn whether_multisig_address(
        &self,
        address: String,
        chain_code: String,
    ) -> Result<AddressStatus, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let mut status = AddressStatus { address_status: 0 };

        if wallet_database::repositories::multisig_account::MultisigAccountRepo::find_doing_account(
            &core_pool,
            &chain_code,
            &address,
        )
        .await?
        .is_some()
        {
            status.address_status = 2;
        }

        if needs_tron_multisig_account_inspection(&chain_code) {
            let adapter =
                ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &chain_code)
                    .await?;
            if let domain::chain::adapter::TransactionAdapter::Tron(tron_chain) = adapter {
                let account = tron_chain.get_provider().account_info(&address).await?;
                if account.is_multisig_account() {
                    status.address_status = 1;
                }
            }
        }
        Ok(status)
    }
}
