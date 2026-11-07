use super::adapter::TransactionAdapter;
use crate::{
    context::Context,
    domain::{bill::BillDomain, coin::CoinDomain},
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    request::transaction::{self, Signer},
};
use wallet_chain_interact::{
    eth,
    sol::{self, SolFeeSetting},
    tron::{TronChain, protocol::account::AccountResourceDetail},
    types::ChainPrivateKey,
};
use wallet_database::{
    entities::{
        account::AccountEntity,
        asset_token_key::AssetTokenKey,
        assets::{AssetsEntity, AssetsId},
        bill::{BillKind, NewBillEntity},
        coin::CoinEntity,
    },
    repositories::{
        account::AccountRepo, assets::AssetsRepo, bill::BillRepo, coin::CoinRepo,
        permission::PermissionRepo,
    },
};
use wallet_transport_backend::{
    api::{BackendApi, wallet::permission::TransPermission},
    consts::endpoint,
    request::PermissionData,
    response_vo::chain::GasOracle,
};
use wallet_types::constant::chain_code;
use wallet_utils::unit;

// sol 默认计算单元
pub const DEFAULT_UNITS: u64 = 100_000;

// 链的交易要单独出来

pub struct ChainTransDomain;

impl ChainTransDomain {
    pub async fn assets(
        ctx: &'static Context,
        chain_code: &str,
        from: &str,
        token_key: AssetTokenKey,
    ) -> Result<AssetsEntity, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        let assets_id = AssetsId {
            address: from.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_key,
        };
        let assets = AssetsRepo::assets_by_id(&pool, &assets_id).await?.ok_or(
            crate::error::business::BusinessError::Assets(
                crate::error::business::assets::AssetsError::NotFound,
            ),
        )?;

        Ok(assets)
    }

    pub async fn account(
        ctx: &'static Context,
        chain_code: &str,
        address: &str,
    ) -> Result<AccountEntity, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let account = AccountRepo::detail_by_address_and_chain_code(pool, address, chain_code)
            .await?
            .ok_or(crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::NotFound(address.to_string()),
            ))?;
        Ok(account)
    }

    pub async fn update_balance(
        ctx: &'static Context,
        address: &str,
        chain_code: &str,
        refresh_symbol: &str,
        token_key: AssetTokenKey,
        balance: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let id = AssetsId::new(address, chain_code, token_key.clone());
        let ticket = ctx.ordinary_balance_order.begin(&id).await;
        Self::update_balance_ordered(
            ctx,
            address,
            chain_code,
            refresh_symbol,
            token_key,
            balance,
            &ticket,
        )
        .await
    }

    pub(crate) async fn update_balance_ordered(
        ctx: &'static Context,
        address: &str,
        chain_code: &str,
        refresh_symbol: &str,
        token_key: AssetTokenKey,
        balance: &str,
        ticket: &crate::domain::assets::refresh_order::Ticket<'_>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let Some(current) = ticket.current().await else { return Ok(()) };
        let pool = ctx.core_pool()?;
        let assets_id = AssetsId::new(address, chain_code, token_key);

        // 查询余额
        let asset = AssetsRepo::assets_by_id(&pool, &assets_id).await?;
        if let Some(asset) = asset {
            // 余额不一致
            if asset.balance != balance {
                // 更新本地余额后在上报后端
                AssetsRepo::update_balance(&pool, &assets_id, balance)
                    .await
                    .map_err(crate::error::service::ServiceError::Database)?;

                drop(current);
                // 上报后端修改余额
                let backend = ctx.get_global_backend_api();
                let rs =
                    backend.wallet_assets_refresh_bal(address, chain_code, refresh_symbol).await;
                if let Err(e) = rs {
                    tracing::warn!("upload balance refresh error = {}", e);
                }
            }
        }

        Ok(())
    }

    pub async fn main_coin(
        ctx: &'static Context,
        chain_code: &str,
    ) -> Result<CoinEntity, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let coin = CoinRepo::main_coin(chain_code, &pool).await?;
        Ok(coin)
    }

    // btc 验证是否存在未确认的交易
    async fn check_ongoing_bill(
        ctx: &'static Context,
        from: &str,
        chain_code: &str,
    ) -> Result<bool, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        if chain_code == chain_code::BTC {
            let res = BillRepo::on_going_bill(chain_code::BTC, from, &pool).await?;
            return Ok(!res.is_empty());
        };

        Ok(false)
    }

    /// transfer
    pub async fn transfer(
        ctx: &'static Context,
        mut params: transaction::TransferReq,
        bill_kind: BillKind,
        adapter: &TransactionAdapter,
        private_key: ChainPrivateKey,
    ) -> Result<String, crate::error::service::ServiceError> {
        //  check ongoing tx
        if Self::check_ongoing_bill(ctx, &params.base.from, &params.base.chain_code).await? {
            return Err(crate::error::business::BusinessError::Bill(
                crate::error::business::bill::BillError::ExistsUnConfirmationTx,
            ))?;
        };

        let coin = CoinDomain::get_coin_by_token_key_with_ctx(
            ctx,
            &params.base.chain_code,
            params.base.token_address.clone(),
        )
        .await?;

        params.base.with_token(coin.token_address.clone());
        params.base.with_decimals(coin.decimals);
        tracing::info!("transfer params = {:?}", params);
        let resp = adapter.transfer(ctx, &mut params, private_key).await?;

        let mut new_bill: NewBillEntity = BillRepo::try_build_bill_from(&params)?;
        new_bill.tx_kind = bill_kind;
        new_bill.hash = resp.tx_hash.clone();
        new_bill.resource_consume = resp.resource_consume()?;
        new_bill.transaction_fee = resp.fee;

        // 如果使用了权限，上报给后端
        if let Some(signer) = params.signer {
            let pool = ctx.core_pool()?;
            let permission = PermissionRepo::permission_with_user(
                &pool,
                &params.base.from,
                signer.permission_id,
                false,
            )
            .await?
            .ok_or(crate::error::business::BusinessError::Permission(
                crate::error::business::permission::PermissionError::ActivesPermissionNotFound,
            ))?;

            let users = permission.users();

            let params = TransPermission {
                address: params.base.from,
                chain_code: params.base.chain_code,
                tx_kind: bill_kind.to_i8(),
                hash: resp.tx_hash.clone(),
                permission_data: PermissionData {
                    opt_address: signer.address.to_string(),
                    users: users.clone(),
                },
            };

            let task = BackendApiTask::BackendApi(BackendApiTaskData::new(
                endpoint::UPLOAD_PERMISSION_TRANS,
                &params,
            )?);
            Tasks::new().push(task).send_with_ctx(ctx).await?;

            new_bill.signer = users;
        }

        let core_pool = ctx.core_pool()?;
        BillDomain::create_bill(&core_pool, new_bill).await?;

        if let Some(request_id) = params.base.request_resource_id {
            let backend = ctx.get_global_backend_api();
            backend.delegate_complete(&request_id).await?;
        }

        Ok(resp.tx_hash)
    }

    pub async fn gas_oracle(
        chain_code: &str,
        provider: &eth::Provider,
        backend: &BackendApi,
    ) -> Result<GasOracle, crate::error::service::ServiceError> {
        Ok(Self::gas_oracle_with_source(chain_code, provider, backend).await?.0)
    }

    pub(crate) async fn gas_oracle_with_source(
        chain_code: &str,
        provider: &eth::Provider,
        backend: &BackendApi,
    ) -> Result<(GasOracle, &'static str), crate::error::service::ServiceError> {
        let gas_oracle = backend.gas_oracle(chain_code).await;
        match gas_oracle {
            Ok(gas_oracle) => Ok((gas_oracle, "backend")),
            Err(_) => {
                // unit is wei need to gwei
                let eth_fee = provider.get_default_fee().await?;

                let propose = eth_fee.base_fee + eth_fee.priority_fee_per_gas;
                let propose = unit::format_to_string(propose, eth::consts::ETH_GWEI)?;
                let base = unit::format_to_string(eth_fee.base_fee, eth::consts::ETH_GWEI)?;

                let gas_oracle = GasOracle {
                    safe_gas_price: None,
                    propose_gas_price: Some(propose),
                    fast_gas_price: None,
                    suggest_base_fee: Some(base),
                    gas_used_ratio: None,
                };

                Ok((gas_oracle, "node"))
            }
        }
    }

    pub async fn default_gas_oracle(
        provider: &eth::Provider,
    ) -> Result<GasOracle, crate::error::service::ServiceError> {
        let eth_fee = provider.get_default_fee().await?;

        let propose = eth_fee.base_fee + eth_fee.priority_fee_per_gas;
        let propose = unit::format_to_string(propose, eth::consts::ETH_GWEI)?;
        let base = unit::format_to_string(eth_fee.base_fee, eth::consts::ETH_GWEI)?;

        let gas_oracle = GasOracle {
            safe_gas_price: None,
            propose_gas_price: Some(propose),
            fast_gas_price: None,
            suggest_base_fee: Some(base),
            gas_used_ratio: None,
        };

        Ok(gas_oracle)
    }

    // check balance
    pub async fn check_eth_balance(
        from: &str,
        balance: alloy::primitives::U256,
        token: Option<&str>,
        chain: &eth::EthChain,
        transfer_amount: alloy::primitives::U256,
    ) -> Result<alloy::primitives::U256, crate::error::service::ServiceError> {
        let cost_main = match token {
            Some(token) => {
                let token_balance = chain.balance(from, Some(token.to_string())).await?;
                if token_balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(from.to_string())
                                .to_addr(from.to_string())
                                .chain_code(chain.chain_code.to_string())
                                .token_addr(token.to_string())
                                .value(transfer_amount.to_string())
                                .balance(token_balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("token balance is insufficient"),
                        ),
                    ))?;
                }
                balance
            }
            None => {
                if balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(from.to_string())
                                .to_addr(from.to_string())
                                .chain_code(chain.chain_code.to_string())
                                .value(transfer_amount.to_string())
                                .balance(balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("main coin balance is insufficient"),
                        ),
                    ))?;
                }
                balance - transfer_amount
            }
        };
        Ok(cost_main)
    }

    // 针对sol 是否需要给优先费计算,目前给到usdt的优先费位 2倍基础费
    pub fn sol_priority_fee(fee_setting: &mut SolFeeSetting, token: Option<&String>, units: u64) {
        if let Some(_token) = token {
            fee_setting.compute_units_consumed = units;
            fee_setting.priority_fee_per_compute_unit = Some(fee_setting.base_fee * 20);
        }
    }

    /// if main coin transfer return reduce transfer amount remain balance
    /// else return balance
    pub async fn check_sol_balance(
        from: &str,
        balance: alloy::primitives::U256,
        token: Option<&str>,
        chain: &sol::SolanaChain,
        transfer_amount: alloy::primitives::U256,
    ) -> Result<alloy::primitives::U256, crate::error::service::ServiceError> {
        let cost_main = match token {
            Some(token) => {
                let token_balance = chain.balance(from, Some(token.to_string())).await?;
                if token_balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(from.to_string())
                                .to_addr(from.to_string())
                                .chain_code("sol")
                                .token_addr(token.to_string())
                                .value(transfer_amount.to_string())
                                .balance(token_balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("token balance is insufficient"),
                        ),
                    ))?;
                }
                balance
            }
            None => {
                if balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(from.to_string())
                                .to_addr(from.to_string())
                                .chain_code("sol")
                                .value(transfer_amount.to_string())
                                .balance(balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("main coin balance is insufficient"),
                        ),
                    ))?;
                }
                balance - transfer_amount
            }
        };
        Ok(cost_main)
    }

    // check transaction_fee
    pub fn check_sol_transaction_fee(
        balance: alloy::primitives::U256,
        fee: u64,
    ) -> Result<(), crate::error::service::ServiceError> {
        let fee = alloy::primitives::U256::from(fee);

        if balance < fee {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientFeeBalance,
            ))?;
        }
        Ok(())
    }

    pub fn handle_btc_fee_error(
        err: wallet_chain_interact::Error,
    ) -> crate::error::service::ServiceError {
        match err {
            wallet_chain_interact::Error::UtxoError(
                wallet_chain_interact::UtxoError::InsufficientBalance,
            ) => crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                    crate::error::business::chain::InsufficientBalanceDetail::new()
                        .reason("utxo balance is insufficient"),
                ),
            )
            .into(),
            wallet_chain_interact::Error::UtxoError(
                wallet_chain_interact::UtxoError::InsufficientFee(_fee),
            ) => crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientFeeBalance,
            )
            .into(),
            wallet_chain_interact::Error::UtxoError(
                wallet_chain_interact::UtxoError::ExceedsMaximum,
            ) => crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::ExceedsMaximum,
            )
            .into(),
            wallet_chain_interact::Error::UtxoError(wallet_chain_interact::UtxoError::DustTx) => {
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::DustTransaction,
                )
                .into()
            }
            wallet_chain_interact::Error::UtxoError(
                wallet_chain_interact::UtxoError::ExceedsMaxFeeRate,
            ) => crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::ExceedsMaxFeerate,
            )
            .into(),
            _ => err.into(),
        }
    }

    // 转账金额不能小于对应币种精度的最小金额,并返回最小金额的u256表示
    pub fn check_min_transfer(
        value: &str,
        decimal: u8,
    ) -> Result<alloy::primitives::U256, crate::error::service::ServiceError> {
        let min = alloy::primitives::U256::from(1);
        let transfer_amount = unit::convert_to_u256(value, decimal)?;

        if transfer_amount < min {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::AmountLessThanMin,
            ))?;
        }
        Ok(transfer_amount)
    }

    // 如果传入了signer 则使用signer的私钥
    pub async fn get_key(
        ctx: &'static Context,
        from: &str,
        chain_code: &str,
        password: &str,
        signer: &Option<Signer>,
    ) -> Result<ChainPrivateKey, crate::error::service::ServiceError> {
        tracing::info!("get_key signer: {:?}", signer);
        let address =
            if let Some(signer) = signer { signer.address.clone() } else { from.to_string() };

        let key =
            crate::domain::account::open_subpk_with_password(ctx, chain_code, &address, password)
                .await?;

        Ok(key)
    }

    // 后期加入缓存
    pub async fn account_resource(
        chain: &TronChain,
        owner_address: &str,
    ) -> Result<AccountResourceDetail, crate::error::service::ServiceError> {
        let resource = chain.account_resource(owner_address).await?;
        Ok(resource)
    }
}
