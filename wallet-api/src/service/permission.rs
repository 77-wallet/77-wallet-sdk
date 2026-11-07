use crate::{
    context::Context,
    domain::{
        self,
        account::open_subpk_with_password,
        chain::adapter::ChainAdapterFactory,
        coin::TokenCurrencyGetter,
        multisig::{MultisigDomain, MultisigQueueDomain},
    },
    messaging::notify::{
        FrontendNotifyEvent,
        event::NotifyEvent,
        other::{Process, TransactionProcessFrontend},
    },
    request::permission::PermissionReq,
    response_vo::{
        EstimateFeeResp, TronFeeDetails,
        standard_wallet::permission::{
            AccountPermission, Keys, ManagerPermissionResp, PermissionList, PermissionResp,
        },
    },
};
use alloy::primitives::map::HashSet;
use wallet_chain_interact::{
    BillResourceConsume,
    tron::{
        TronChain, consts,
        operations::{TronTxOperation, multisig::Permission, permissions::PermissionUpdateArgs},
    },
};
use wallet_database::{
    CoreDbPool,
    entities::{
        asset_token_key::AssetTokenKey, bill::BillKind, multisig_queue::NewMultisigQueueEntity,
    },
    repositories::{address_book::AddressBookRepo, bill::BillRepo, permission::PermissionRepo},
};
use wallet_transport_backend::api::wallet::permission::PermissionAcceptReq;
use wallet_types::constant::chain_code;

pub struct PermissionService {
    ctx: &'static Context,
    chain: TronChain,
}

impl PermissionService {
    pub async fn new(ctx: &'static Context) -> Result<Self, crate::error::service::ServiceError> {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        Ok(Self { ctx, chain })
    }

    // 标记使用地址簿里面的名字
    pub async fn mark_address_book_name(
        &self,
        pool: &CoreDbPool,
        keys: &mut [Keys],
    ) -> Result<(), crate::error::service::ServiceError> {
        for key in keys.iter_mut() {
            let book = AddressBookRepo::find_by_address_chain(pool, &key.address, chain_code::TRON)
                .await?;
            if let Some(book) = book {
                key.name = book.name;
            }
        }

        Ok(())
    }

    async fn update_permission<T>(
        &self,
        from: &str,
        args: impl TronTxOperation<T>,
        password: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            BillKind::UpdatePermission,
            Process::Building,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;

        // 手续拦截
        let resp = args.build_raw_transaction(&self.chain.provider).await?;
        // 验证余额
        let balance = self.chain.balance(from, None).await?;
        let mut consumer =
            self.chain.get_provider().transfer_fee(from, None, &resp.raw_data_hex, 1).await?;

        // upgrade fee
        consumer.set_extra_fee(100 * consts::TRX_VALUE);

        if balance.to::<i64>() < consumer.transaction_fee_i64() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::InsufficientFeeBalance,
                ),
            ));
        }

        // 广播交易交易事件
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            BillKind::UpdatePermission,
            Process::Broadcast,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;

        let key = open_subpk_with_password(self.ctx, chain_code::TRON, from, password).await?;
        let hash = self.chain.exec_transaction_v1(resp, key).await?;

        let transaction_fee = consumer.transaction_fee();
        // 写入本地交易数据

        let bill_consumer = BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0);
        let entity = BillRepo::build_stake_bill(
            hash.clone(),
            from.to_string(),
            args.get_to(),
            args.get_value(),
            BillKind::UpdatePermission,
            bill_consumer.to_json_str()?,
            transaction_fee,
            None::<String>,
        );
        let pool = self.ctx.core_pool()?;
        domain::bill::BillDomain::create_bill(&pool, entity).await?;

        Ok(hash)
    }

    async fn update_permission_fee<T: std::fmt::Debug + serde::Serialize>(
        &self,
        from: &str,
        args: impl TronTxOperation<T>,
    ) -> Result<EstimateFeeResp, crate::error::service::ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();
        let token_currency = TokenCurrencyGetter::get_currency_by_token_key(
            self.ctx,
            currency,
            "tron",
            "TRX",
            AssetTokenKey::Native,
        )
        .await?;

        // 预估手续费
        let mut consumer = self.chain.simple_fee(from, 1, args).await?;
        let chain_parameter = self.chain.provider.chain_params().await?;
        consumer.set_extra_fee(chain_parameter.update_account_fee());

        let res = TronFeeDetails::new(consumer, token_currency, currency)?;
        let content = wallet_utils::serde_func::serde_to_string(&res)?;

        Ok(EstimateFeeResp::new("TRX".to_string(), chain_code::TRON.to_string(), content))
    }

    // 上报后端
    async fn upload_backend(
        &self,
        params: PermissionAcceptReq,
    ) -> Result<(), crate::error::service::ServiceError> {
        let backend = self.ctx.get_global_backend_api();
        Ok(backend.permission_accept(params).await?)
    }
}

impl PermissionService {
    // all permission category
    pub fn permission_list() -> Result<PermissionList, crate::error::service::ServiceError> {
        Ok(PermissionList::default())
    }

    // account permission
    pub async fn account_permission(
        &self,
        address: String,
    ) -> Result<Option<AccountPermission>, crate::error::service::ServiceError> {
        let account = self.chain.account_info(&address).await?;
        if account.address.is_empty() {
            return Ok(None);
        }

        let actives = account
            .active_permission
            .iter()
            .map(PermissionResp::try_from)
            .collect::<Result<Vec<PermissionResp>, _>>()?;

        let mut result = AccountPermission {
            owner: PermissionResp::try_from(&account.owner_permission)?,
            actives,
        };

        let pool = self.ctx.core_pool()?;

        self.mark_address_book_name(&pool, &mut result.owner.keys).await?;

        for item in result.actives.iter_mut() {
            self.mark_address_book_name(&pool, &mut item.keys).await?;
        }

        Ok(Some(result))
    }

    // 我管理的权限
    pub async fn manager_permission(
        &self,
        grantor_addr: String,
    ) -> Result<Vec<ManagerPermissionResp>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;

        let permissions =
            PermissionRepo::all_permission_with_user(&core_pool, &grantor_addr).await?;

        let mut result: Vec<ManagerPermissionResp> = vec![];

        for permission in permissions.iter() {
            let grantor_addr = permission.permission.grantor_addr.clone();
            let name = permission.permission.name.clone();
            let mut p = PermissionResp::try_from(permission)?;

            self.mark_address_book_name(&core_pool, &mut p.keys).await?;

            result.push(ManagerPermissionResp { grantor_addr, name, permission: p });
        }

        Ok(result)
    }

    // 构建参数
    fn build_args(
        &self,
        args: &mut PermissionUpdateArgs,
        types: &str,
        req: &PermissionReq,
        backup_params: Option<&mut PermissionAcceptReq>,
    ) -> Result<(), crate::error::service::ServiceError> {
        req.check_threshold()?;
        match types {
            PermissionReq::NEW => {
                if args.actives.len() > 7 {
                    return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::Permission(
                        crate::error::business::permission::PermissionError::ActivesPermissionMore,
                    )));
                };

                let permission = Permission::try_from(req)?;

                // 拼接上报后端的参数
                if let Some(params) = backup_params {
                    params.current.types = PermissionReq::NEW.to_string();
                    params.current.name = req.name.to_string();
                    params.current.active_id = args.actives.len() as i64 + 2;
                    params.current.new_user = req.users();
                    params.current.operations = permission.operations.clone().unwrap_or_default();

                    params.sender_user = req.users();
                }

                args.actives.push(permission);
                Ok(())
            }
            PermissionReq::UPDATE => {
                // 对原来的进行调整
                let id = req.active_id.unwrap_or_default();
                let new_permission = Permission::try_from(req)?;

                if let Some(permission) =
                    args.actives.iter_mut().find(|p| p.id.unwrap_or_default() == id)
                {
                    // 拼接上报后端的参数
                    if let Some(params) = backup_params {
                        let mut users = permission.users_from_hex()?;

                        params.current.original_user = users.clone();
                        params.current.types = PermissionReq::UPDATE.to_string();
                        params.current.name = req.name.to_string();
                        params.current.active_id = id as i64;
                        params.current.new_user = req.users();
                        params.current.operations =
                            permission.operations.clone().unwrap_or_default();

                        users.extend(req.users());

                        params.sender_user =
                            users.into_iter().collect::<HashSet<String>>().into_iter().collect();
                    }

                    *permission = new_permission;
                } else {
                    return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::Permission(
                        crate::error::business::permission::PermissionError::ActivesPermissionNotFound,
                    )));
                }

                Ok(())
            }
            PermissionReq::DELETE => {
                let active_id = req.active_id.unwrap_or_default();
                // check exists
                let permission =
                    args.actives.iter().find(|a| a.id.unwrap_or_default() == active_id).cloned();

                if let Some(permission) = permission {
                    // 拼接上报后端的参数
                    if let Some(params) = backup_params {
                        let users = permission.users_from_hex()?;

                        params.current.original_user = users.clone();
                        params.current.types = PermissionReq::DELETE.to_string();
                        params.current.name = req.name.to_string();
                        params.current.active_id = active_id as i64;
                    }

                    // 删除权限
                    args.actives.retain(|item| item.id.unwrap_or_default() != active_id);

                    if args.actives.is_empty() {
                        return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::Permission(
                            crate::error::business::permission::PermissionError::MissActivesPermission,
                        )));
                    }

                    Ok(())
                } else {
                    return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::Permission(
                        crate::error::business::permission::PermissionError::ActivesPermissionNotFound,
                    )));
                }
            }
            _ => Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Permission(
                    crate::error::business::permission::PermissionError::UnSupportOpType(
                        types.to_string(),
                    ),
                ),
            )),
        }
    }

    pub async fn modify_permission_fee(
        &self,
        req: PermissionReq,
        types: String,
    ) -> Result<EstimateFeeResp, crate::error::service::ServiceError> {
        // 构建公用的参数
        let account = self.chain.account_info(&req.grantor_addr).await?;
        let mut args = PermissionUpdateArgs::try_from(&account)?;

        self.build_args(&mut args, &types, &req, None)?;

        self.update_permission_fee(&req.grantor_addr, args).await
    }

    pub async fn modify_permission(
        &self,
        req: PermissionReq,
        types: String,
        password: String,
    ) -> Result<String, crate::error::service::ServiceError> {
        // 构建公用的参数
        let account = self.chain.account_info(&req.grantor_addr).await?;
        let mut args = PermissionUpdateArgs::try_from(&account)?;

        // 上报后端的参数
        let mut backend_params = PermissionAcceptReq::default();
        if types == PermissionReq::DELETE {
            backend_params.sender_user = account.all_actives_user();
        }

        self.build_args(&mut args, &types, &req, Some(&mut backend_params))?;

        // 这个地址所有权限的用户集合
        let mut new_users = HashSet::new();
        for item in args.actives.iter() {
            for key in item.keys.iter() {
                new_users.insert(wallet_utils::address::hex_to_bs58_addr(&key.address)?);
            }
        }

        let tx_hash = self.update_permission(&req.grantor_addr, args, &password).await?;

        backend_params.hash = tx_hash.clone();
        backend_params.grantor_addr = req.grantor_addr.clone();
        backend_params.back_user = new_users.into_iter().collect();

        // 上报后端
        self.upload_backend(backend_params).await?;

        Ok(tx_hash)
    }

    pub async fn build_multisig_permission(
        &self,
        req: PermissionReq,
        types: String,
        expiration: i64,
        password: String,
    ) -> Result<String, crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let bill_kind = BillKind::UpdatePermission;

        let account = MultisigDomain::account_by_address(&req.grantor_addr, true, &pool).await?;
        MultisigDomain::validate_queue(&account)?;

        let tron_account = self.chain.account_info(&req.grantor_addr).await?;
        let mut backend_params = PermissionAcceptReq::default();
        if types == PermissionReq::DELETE {
            backend_params.sender_user = tron_account.all_actives_user();
        }

        let mut args = PermissionUpdateArgs::try_from(&tron_account)?;
        self.build_args(&mut args, &types, &req, Some(&mut backend_params))?;

        // 这个地址所有权限的用户集合
        let mut new_users = HashSet::new();
        for item in args.actives.iter() {
            for key in item.keys.iter() {
                new_users.insert(wallet_utils::address::hex_to_bs58_addr(&key.address)?);
            }
        }

        // 构建多签交易
        let expiration = MultisigQueueDomain::sub_expiration(expiration);
        let resp = self.chain.build_multisig_transaction(args, expiration as u64).await?;

        let mut queue = NewMultisigQueueEntity::new(
            account.id.to_string(),
            req.grantor_addr.to_string(),
            String::new(),
            expiration,
            &resp.tx_hash,
            &resp.raw_data,
            bill_kind,
            "0".to_string(),
        );

        let res = MultisigQueueDomain::tron_sign_and_create_queue(
            self.ctx,
            &mut queue,
            &account,
            password,
            pool.clone(),
        )
        .await?;

        backend_params.grantor_addr = req.grantor_addr.clone();
        backend_params.back_user = new_users.into_iter().collect();
        backend_params.multi_sign_id = res.id.clone();

        MultisigQueueDomain::upload_queue_backend(
            self.ctx,
            res.id,
            &pool,
            Some(backend_params),
            None,
        )
        .await?;

        Ok(resp.tx_hash)
    }
}
