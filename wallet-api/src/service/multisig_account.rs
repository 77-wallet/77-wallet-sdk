use crate::domain;
use crate::domain::task_queue::Tasks;
use crate::mqtt::payload::incoming::signature::OrderMultiSignAccept;
use crate::request::transaction;
use crate::response_vo;
use crate::response_vo::multisig_account::{
    AddressStatus, MultisigAccountInfo, MultisigAccountList, MultisigFeeVo,
};
use std::collections::HashMap;
use wallet_database::entities::assets::AssetsEntity;
use wallet_database::entities::bill::{BillKind, NewBillEntity};
use wallet_database::entities::chain::ChainEntity;
use wallet_database::entities::coin::CoinMultisigStatus;
use wallet_database::entities::multisig_account::{
    MultiAccountOwner, MultisigAccountEntity, MultisigAccountPayStatus, MultisigAccountStatus,
    NewMultisigAccountEntity,
};
use wallet_database::entities::multisig_member::{MemberVo, MultisigMemberEntities};
use wallet_database::entities::wallet::WalletEntity;
use wallet_database::pagination::Pagination;
use wallet_transport_backend::consts::endpoint;
use wallet_transport_backend::{
    api::BackendApi,
    request::{SignedFeeListReq, SignedFindAddressReq},
    DepositAddress, SignedOrderAcceptReq, SignedSaveAddressReq, SignedUpdateRechargeHashReq,
    SignedUpdateSignedHashReq,
};
use wallet_transport_backend::{ConfirmedAddress, OrderMultisigUpdateArg, SingedOrderCancelReq};
use wallet_types::chain::address::category::BtcAddressCategory;
use wallet_types::chain::address::r#type::BtcAddressType;
use wallet_types::constant::chain_code;
use wallet_utils::serde_func;

pub struct MultisigAccountService {
    repo: wallet_database::repositories::multisig_account::MultisigAccountRepo,
    backend: BackendApi,
}

impl MultisigAccountService {
    pub fn new(
        repo: wallet_database::repositories::multisig_account::MultisigAccountRepo,
    ) -> Result<Self, crate::ServiceError> {
        let backend = crate::manager::Context::get_global_backend_api()?.clone();
        Ok(Self { repo, backend })
    }

    pub async fn crate_account(
        &self,
        name: String,
        address: String,
        chain_code: String,
        threshold: i32,
        mut member_list: Vec<MemberVo>,
        address_type: Option<String>,
    ) -> Result<(), crate::ServiceError> {
        // check address type
        let address_type = match chain_code.as_str() {
            chain_code::BTC => {
                let address_type = address_type.ok_or(crate::BusinessError::Chain(
                    crate::ChainError::BitcoinAddressEmpty,
                ))?;
                let category = BtcAddressCategory::try_from(address_type)?;
                BtcAddressType::from(category).to_string()
            }
            _ => String::new(),
        };

        // check whether has not complete account.
        if self
            .repo
            .find_doing_account(&chain_code, &address)
            .await?
            .is_some()
        {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::HasUncompletedAccount,
            ))?;
        }
        let member_address = member_list
            .iter()
            .map(|m| m.address.clone())
            .collect::<Vec<String>>();

        // 获取address对应的uid
        let address_uid = self
            .backend
            .get_address_uid(chain_code.clone(), member_address)
            .await?;
        // 设置member 的ui
        for item in member_list.iter_mut() {
            if item.uid.is_empty() {
                match address_uid
                    .list
                    .iter()
                    .find(|uid| item.address == uid.address)
                {
                    Some(find) if !find.uid.is_empty() => {
                        item.uid = find.uid.clone();
                    }
                    _ => {
                        return Err(crate::BusinessError::MultisigAccount(
                            crate::MultisigAccountError::NotPlatFormAddress,
                        ))?;
                    }
                }
            }
        }

        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let uid_list = WalletEntity::uid_list(&*pool)
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect();

        let mut params = NewMultisigAccountEntity::new(
            None,
            name,
            address.clone(),
            address,
            chain_code,
            threshold,
            address_type,
            member_list,
            &uid_list,
        );

        self.multisig_account_name(&mut params).await?;
        self.mark_self_account(&mut params).await?;

        let raw_data = params.to_multisig_account_data().to_string()?;
        let sync_account_params = OrderMultiSignAccept::from(&params);
        let req = SignedSaveAddressReq::new(
            &params.id,
            &params.chain_code,
            &params.initiator_addr,
            sync_account_params.member_lists(),
            &sync_account_params.to_json_str()?,
            raw_data,
        );
        self.backend.signed_order_save_confirm_address(req).await?;
        self.repo.create_with_member(&params).await?;

        Ok(())
    }

    async fn multisig_account_name(
        &self,
        params: &mut NewMultisigAccountEntity,
    ) -> Result<(), crate::ServiceError> {
        if !params.name.is_empty() {
            return Ok(());
        }

        let count = self.repo.account_count(&params.chain_code).await;
        params.name = format!("Multisig-{}-{}", params.chain_code, count + 1);

        Ok(())
    }

    async fn mark_self_account(
        &self,
        params: &mut NewMultisigAccountEntity,
    ) -> Result<(), crate::ServiceError> {
        let mut flag = true;

        for item in params.member_list.iter_mut() {
            if let Some(account) = self
                .repo
                .wallet_account(&item.address, &params.chain_code)
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
        &self,
        id: &str,
    ) -> Result<Option<MultisigAccountInfo>, crate::ServiceError> {
        let account = self.repo.found_by_id(id).await?;

        let mut account = match account {
            Some(account) => account,
            None => return Ok(None),
        };

        account.address_type_to_category();
        let member = self.repo.member_by_account_id(&account.id).await?.0;

        Ok(Some(MultisigAccountInfo { account, member }))
    }

    pub async fn multisig_account_by_address(
        &self,
        address: &str,
    ) -> Result<Option<MultisigAccountInfo>, crate::ServiceError> {
        let account = self.repo.found_by_address(address).await?;

        let mut account = match account {
            Some(account) => account,
            None => return Ok(None),
        };

        account.address_type_to_category();
        let member = self.repo.member_by_account_id(&account.id).await?.0;
        Ok(Some(MultisigAccountInfo { account, member }))
    }

    pub async fn update_multisig_name(
        &self,
        account_id: String,
        name: String,
    ) -> Result<(), crate::ServiceError> {
        Ok(self.repo.update_name(&account_id, &name).await?)
    }

    pub async fn cancel_multisig(&self, id: String) -> Result<(), crate::ServiceError> {
        let account =
            self.repo
                .found_by_id(&id)
                .await?
                .ok_or(crate::BusinessError::MultisigAccount(
                    crate::MultisigAccountError::NotFound,
                ))?;

        if account.status == MultisigAccountStatus::OnChain.to_i8() {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::CannotCancel,
            ))?;
        }

        self.repo.cancel_multisig(&account).await?;

        // 上报后端
        let raw_data = self.repo.multisig_data(&account.id).await?.to_string()?;
        let req = SingedOrderCancelReq {
            order_id: account.id.clone(),
            raw_data,
        };
        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData::new(
                    endpoint::multisig::SIGNED_ORDER_CANCEL,
                    &req,
                )?,
            ));
        Tasks::new().push(task).send().await?;

        Ok(())
    }

    pub async fn account_list(
        &self,
        owner: bool,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigAccountList>, crate::ServiceError> {
        let mut res = self
            .repo
            .account_list(owner, chain_code, page, page_size)
            .await?;

        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let mut list = vec![];
        // main symbol
        for item in res.data.iter_mut() {
            let chain = ChainEntity::detail(&*pool, &item.chain_code)
                .await
                .unwrap()
                .unwrap();
            item.address_type_to_category();
            list.push({
                MultisigAccountList {
                    account: item.clone(),
                    symbol: chain.main_symbol,
                }
            });
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
        &self,
        chain_code: &str,
    ) -> Result<DepositAddress, crate::ServiceError> {
        let req = SignedFindAddressReq::new(chain_code);
        self.backend
            .signed_find_address(req)
            .await
            .map_err(crate::ServiceError::TransportBackend)
    }

    pub async fn get_multisig_service_fee(
        &self,
        chain_code: &str,
    ) -> Result<MultisigFeeVo, crate::ServiceError> {
        // service fee
        let req = SignedFeeListReq::new(chain_code);
        let res = self.backend.signed_fee_list(req).await?;

        if res.list.is_empty() {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::ServiceFeeNotConfig,
            ))?;
        }

        let fee = res.list.first().unwrap();
        // address
        let req = SignedFindAddressReq::new(chain_code);
        let res = self.backend.signed_find_address(req).await?;

        let fee = MultisigFeeVo {
            symbol: "USDT".to_string(),
            fee: fee.price.to_string(),
            address: res.address,
        };
        Ok(fee)
    }

    pub async fn check_participant_exists(
        &self,
        account_id: String,
    ) -> Result<Vec<String>, crate::ServiceError> {
        let multisig_account = self.repo.found_by_id(&account_id).await?.ok_or(
            crate::BusinessError::MultisigAccount(crate::MultisigAccountError::NotFound),
        )?;

        // only my address
        let member = self.repo.self_address_by_id(&account_id).await?;

        let mut not_exits = vec![];

        for m in member.0 {
            let account = self
                .repo
                .wallet_account(&m.address, &multisig_account.chain_code)
                .await?;
            if account.is_none() {
                not_exits.push(m.address);
            }
        }

        Ok(not_exits)
    }

    pub async fn confirm_participation(&self, id: &str) -> Result<(), crate::ServiceError> {
        let multisig_account =
            self.repo
                .found_by_id(id)
                .await?
                .ok_or(crate::BusinessError::MultisigAccount(
                    crate::MultisigAccountError::NotFound,
                ))?;

        if multisig_account.is_del == 1 {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::IsCancel,
            ))?;
        }

        // only my address
        let mut self_address = self.repo.self_address_by_id(id).await?;

        // do update confirm status
        self.repo
            .update_confirm_status(
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

        let raw_data = self.repo.multisig_data(id).await?.to_string()?;
        let req = SignedOrderAcceptReq {
            order_id: multisig_account.id,
            accept_address,
            status: 1,
            raw_data,
        };
        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData {
                    endpoint: endpoint::multisig::SIGNED_ORDER_ACCEPT.to_string(),
                    body: serde_func::serde_to_value(&req)?,
                },
            ));
        Tasks::new().push(task).send().await?;

        Ok(())
    }

    pub async fn deploy_multisig_account(
        &self,
        account_id: &str,
        deploy_fee: Option<String>,
        payer: Option<transaction::ServiceFeePayer>,
        password: &str,
    ) -> Result<(), crate::ServiceError> {
        let multisig_account = self.repo.found_by_id(account_id).await?.ok_or(
            crate::BusinessError::MultisigAccount(crate::MultisigAccountError::NotFound),
        )?;

        // service fee
        if multisig_account.pay_status != MultisigAccountPayStatus::Paid.to_i8() {
            if let Some(payer) = payer {
                let fee_chain = payer.chain_code.clone();
                let fee_hash = self
                    ._transfer_service_fee(&multisig_account, payer, password)
                    .await?;

                // 同步多签账户里面的fee_hash,以及费用是部署在那个链上的
                let params = HashMap::from([
                    ("fee_hash".to_string(), fee_hash),
                    ("fee_chain".to_string(), fee_chain),
                    (
                        "pay_status".to_string(),
                        MultisigAccountPayStatus::PaidPending.to_i8().to_string(),
                    ),
                ]);
                let _ = self.repo.update_by_id(account_id, params).await?;
            } else {
                return Err(crate::BusinessError::MultisigAccount(
                    crate::MultisigAccountError::PayerNeed,
                ))?;
            };
        }

        // deploy account
        if multisig_account.status != MultisigAccountStatus::OnChain.to_i8() {
            let member = self.repo.member_by_account_id(&multisig_account.id).await?;

            let multisig_adapter =
                domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(
                    &multisig_account.chain_code,
                )
                .await?;

            // 有交易hash验证是否成功，如果已经成功了不在重复部署
            if !multisig_account.deploy_hash.is_empty() {
                let tx = multisig_adapter
                    .query_tx_res(&multisig_account.deploy_hash)
                    .await?;
                if let Some(tx) = tx {
                    if tx.status != 3 {
                        return Ok(());
                    }
                }
            }

            let resp = multisig_adapter
                .multisig_address(&multisig_account, &member)
                .await?;

            // if chain_code is bitcoin the status is success on chain
            let status = if multisig_account.chain_code == chain_code::BTC {
                MultisigAccountStatus::OnChain.to_i8().to_string()
            } else {
                MultisigAccountStatus::OnChianPending.to_i8().to_string()
            };

            let hash_map = HashMap::from([
                ("address".to_string(), resp.multisig_address.clone()),
                ("salt".to_string(), resp.salt.clone()),
                ("authority_addr".to_string(), resp.authority_address.clone()),
            ]);
            let multisig_account = self
                .repo
                .update_by_id(&multisig_account.id, hash_map)
                .await?;

            let pool = crate::manager::Context::get_global_sqlite_pool()?;

            // 初始化默认资产资产(发起方如果是波场的情况单独处理,将这个地址的其他资产也同步为多签的)
            domain::assets::AssetsDomain::init_default_multisig_assets(
                resp.multisig_address.clone(),
                multisig_account.chain_code.clone(),
            )
            .await?;
            if multisig_account.chain_code.as_str() == chain_code::TRON {
                AssetsEntity::update_tron_multisig_assets(
                    &resp.multisig_address,
                    &multisig_account.chain_code,
                    CoinMultisigStatus::IsMultisig.to_i8(),
                    &*pool,
                )
                .await?;
            };

            // 部署多签账户到链上
            let hash = self
                ._deploy_account(
                    multisig_account,
                    &member,
                    deploy_fee,
                    password,
                    &multisig_adapter,
                )
                .await?;

            let params = HashMap::from([
                ("deploy_hash".to_string(), hash),
                ("status".to_string(), status),
            ]);
            let _ = self.repo.update_by_id(account_id, params).await?;
        }
        Ok(())
    }

    async fn _transfer_service_fee(
        &self,
        multisig_account: &MultisigAccountEntity,
        payer: transaction::ServiceFeePayer,
        password: &str,
    ) -> Result<String, crate::ServiceError> {
        let backend = crate::manager::Context::get_global_backend_api()?;

        // fetch address
        let req = SignedFindAddressReq::new(&payer.chain_code);
        let address = backend.signed_find_address(req).await?;
        let to = &address.address;

        // fetch value
        let req = SignedFeeListReq::new(&payer.chain_code);
        let amount = backend.signed_fee_list(req).await?;
        let amount = amount.list.first().unwrap();
        let value = amount.price.to_string();

        // transfer parameter
        let base = transaction::BaseTransferReq::new(
            payer.from,
            to.to_string(),
            value.clone(),
            payer.chain_code.clone(),
            payer.symbol,
        );
        let params = transaction::TransferReq {
            base,
            password: password.to_string(),
            fee_setting: payer.fee_setting.unwrap_or_default(),
        };

        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(&payer.chain_code)
                .await?;
        // 如果交易hash存在，验证交易是否成功了避免重复
        if !multisig_account.fee_hash.is_empty() {
            let tx = adapter.query_tx_res(&multisig_account.fee_hash).await?;
            if let Some(tx) = tx {
                if tx.status != 3 {
                    // 交易成功或者在确认中不在进行交易，只有失败才再次转账
                    return Ok(multisig_account.fee_hash.clone());
                }
            }
        }

        let tx_hash = domain::chain::transaction::ChainTransaction::transfer(
            params,
            BillKind::ServiceCharge,
            &adapter,
        )
        .await?;

        // sync to backend
        let mut raw_data = self.repo.multisig_data(&multisig_account.id).await?;
        raw_data.account.fee_chain = payer.chain_code.clone();

        let req = SignedUpdateRechargeHashReq {
            order_id: multisig_account.id.to_string(),
            hash: tx_hash.clone(),
            product_code: amount.code.clone(),
            receive_chain_code: payer.chain_code,
            receive_address: to.to_string(),
            raw_data: raw_data.to_string()?,
        };
        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData {
                    endpoint: endpoint::multisig::SIGNED_ORDER_UPDATE_RECHARGE_HASH.to_string(),
                    body: serde_func::serde_to_value(&req)?,
                },
            ));
        Tasks::new().push(task).send().await?;

        Ok(tx_hash)
    }

    async fn _deploy_account(
        &self,
        account: MultisigAccountEntity,
        members: &MultisigMemberEntities,
        deploy_fee: Option<String>,
        password: &str,
        adapter: &domain::chain::adapter::MultisigAdapter,
    ) -> Result<String, crate::ServiceError> {
        // 1.执行链上部署交易
        let key = domain::account::open_account_pk_with_password(
            &account.chain_code,
            &account.initiator_addr,
            password,
        )
        .await?;

        let (hash, bill_consumer) = adapter
            .deploy_multisig_account(&account, members, deploy_fee, key)
            .await?;

        // 2. 不是btc的创建一个部署的bill
        if account.chain_code != chain_code::BTC {
            let main_coin =
                domain::chain::transaction::ChainTransaction::main_coin(&account.chain_code)
                    .await?;
            let mut new_bill = NewBillEntity::new_deploy_bill(
                hash.clone(),
                account.initiator_addr.clone(),
                main_coin.chain_code,
                main_coin.symbol,
            );
            new_bill.resource_consume = bill_consumer;
            crate::domain::bill::BillDomain::create_bill(new_bill).await?;
        }

        // 3.同步后端数据(used to sync other member update data)
        let multisig_args = OrderMultisigUpdateArg {
            multisig_account_id: account.id.clone(),
            multisig_account_address: account.address.clone(),
            address_type: account.address_type.clone(),
            salt: account.salt.clone(),
            authority_addr: account.authority_addr.clone(),
        };

        let mut raw_data = self.repo.multisig_data(&account.id).await?;
        raw_data.account.deploy_hash = hash.clone();

        let req = SignedUpdateSignedHashReq::new(
            &account.id,
            &hash,
            &account.address,
            &account.salt,
            multisig_args.to_json_str()?,
            raw_data.to_string()?,
        );
        let task =
            domain::task_queue::Task::BackendApi(domain::task_queue::BackendApiTask::BackendApi(
                domain::task_queue::BackendApiTaskData {
                    endpoint: endpoint::multisig::SIGNED_ORDER_UPDATE_SIGNED_HASH.to_string(),
                    body: serde_func::serde_to_value(&req)?,
                },
            ));
        Tasks::new().push(task).send().await?;

        Ok(hash)
    }

    pub async fn deploy_multisig_fee(
        &self,
        account_id: &str,
    ) -> Result<response_vo::EstimateFeeResp, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let account =
            domain::multisig::MultisigDomain::account_by_id(account_id, pool.clone()).await?;

        let main_coin =
            domain::chain::transaction::ChainTransaction::main_coin(&account.chain_code).await?;

        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_multisig_adapter(&account.chain_code)
                .await?;

        let member = self.repo.member_by_account_id(account_id).await?;

        let fee = adapter
            .deploy_multisig_fee(&account, member, &main_coin.symbol)
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
    ) -> Result<AddressStatus, crate::ServiceError> {
        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(&chain_code)
                .await?;

        let mut status = AddressStatus { address_status: 0 };

        if self
            .repo
            .find_doing_account(&chain_code, &address)
            .await?
            .is_some()
        {
            status.address_status = 2;
        }

        if let domain::chain::adapter::TransactionAdapter::Tron(tron_chain) = adapter {
            let account = tron_chain.get_provider().account_info(&address).await?;
            if account.is_multisig_account() {
                status.address_status = 1;
            }
        };
        Ok(status)
    }
}
