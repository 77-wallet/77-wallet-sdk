use super::adapter::TransactionAdapter;
use crate::request::transaction;
use wallet_chain_interact::{eth, sol};
use wallet_database::entities::{
    account::AccountEntity,
    assets::{AssetsEntity, AssetsId},
    bill::BillKind,
    coin::CoinEntity,
};
use wallet_transport_backend::{api::BackendApi, response_vo::chain::GasOracle};
use wallet_utils::unit;

pub struct ChainTransaction;

impl ChainTransaction {
    pub async fn assets(
        chain_code: &str,
        symbol: &str,
        from: &str,
    ) -> Result<AssetsEntity, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let assets_id = AssetsId {
            address: from.to_string(),
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
        };
        let assets = AssetsEntity::assets_by_id(&*pool, &assets_id)
            .await?
            .ok_or(crate::BusinessError::Assets(crate::AssetsError::NotFound))?;

        Ok(assets)
    }

    pub async fn account(
        chain_code: &str,
        address: &str,
    ) -> Result<AccountEntity, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let account =
            AccountEntity::find_one_by_address_chain_code(address, chain_code, pool.as_ref())
                .await?
                .ok_or(crate::BusinessError::Account(crate::AccountError::NotFound))?;
        Ok(account)
    }

    pub async fn update_balance(
        address: &str,
        chain_code: &str,
        symbol: &str,
        balance: &str,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let assets_id = AssetsId {
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
        };

        Ok(AssetsEntity::update_balance(&*pool, &assets_id, balance)
            .await
            .map_err(crate::SystemError::Database)?)
    }

    pub async fn main_coin(chain_code: &str) -> Result<CoinEntity, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let coin = CoinEntity::main_coin(chain_code, pool.as_ref())
            .await?
            .ok_or(crate::BusinessError::Coin(
                crate::error::business::coin::CoinError::NotFound(format!(
                    "chian = {}",
                    chain_code
                )),
            ))?;
        Ok(coin)
    }

    /// transfer
    pub async fn transfer(
        mut params: transaction::TransferReq,
        bill_kind: BillKind,
        adapter: &TransactionAdapter,
    ) -> Result<String, crate::ServiceError> {
        // get private_key
        let private_key = crate::domain::account::open_account_pk_with_password(
            &params.base.chain_code,
            &params.base.from,
            &params.password,
        )
        .await?;

        // user assets
        let assets = ChainTransaction::assets(
            &params.base.chain_code,
            &params.base.symbol,
            &params.base.from,
        )
        .await?;
        params.base.with_token(assets.token_address());
        params.base.with_decimals(assets.decimals);

        let resp = adapter.transfer(&params, private_key).await?;

        let mut new_bill = wallet_database::entities::bill::NewBillEntity::try_from(&params)?;
        new_bill.tx_kind = bill_kind;
        new_bill.hash = resp.tx_hash.clone();
        new_bill.resource_consume = resp.resource_consume()?;
        new_bill.transaction_fee = resp.fee;

        crate::domain::bill::BillDomain::create_bill(new_bill).await?;

        if let Some(request_id) = params.base.request_resource_id {
            let backend = crate::manager::Context::get_global_backend_api()?;
            let _ = backend.delegate_complete(&request_id).await;
        }
        Ok(resp.tx_hash)
    }

    pub async fn gas_oracle(
        chain_code: &str,
        provider: &eth::Provider,
        backend: &BackendApi,
    ) -> Result<GasOracle, crate::ServiceError> {
        let gas_oracle = backend.gas_oracle(chain_code).await;

        match gas_oracle {
            Ok(gas_oracle) => Ok(gas_oracle),
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

                Ok(gas_oracle)
            }
        }
    }

    // check balance
    pub async fn check_eth_balance(
        from: &str,
        balance: alloy::primitives::U256,
        token: Option<&str>,
        chain: &eth::EthChain,
        transfer_amount: alloy::primitives::U256,
    ) -> Result<alloy::primitives::U256, crate::ServiceError> {
        let cost_main = match token {
            Some(token) => {
                let token_balance = chain.balance(from, Some(token.to_string())).await?;
                if token_balance < transfer_amount {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }
                balance
            }
            None => {
                if balance < transfer_amount {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }
                balance - transfer_amount
            }
        };
        Ok(cost_main)
    }

    /// if main coin transfer return reduce transfer amount remain balance
    /// else return balance
    pub async fn check_sol_balance(
        from: &str,
        balance: alloy::primitives::U256,
        token: Option<&str>,
        chain: &sol::SolanaChain,
        transfer_amount: alloy::primitives::U256,
    ) -> Result<alloy::primitives::U256, crate::ServiceError> {
        let cost_main = match token {
            Some(token) => {
                let token_balance = chain.balance(from, Some(token.to_string())).await?;
                if token_balance < transfer_amount {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }
                balance
            }
            None => {
                if balance < transfer_amount {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
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
    ) -> Result<(), crate::ServiceError> {
        let fee = alloy::primitives::U256::from(fee);

        if balance < fee {
            return Err(crate::BusinessError::Chain(
                crate::ChainError::InsufficientFeeBalance,
            ))?;
        }
        Ok(())
    }

    pub fn handle_btc_fee_error(err: wallet_chain_interact::Error) -> crate::ServiceError {
        match err {
            wallet_chain_interact::Error::UtxoError(
                wallet_chain_interact::UtxoError::InsufficientBalance,
            ) => crate::BusinessError::Chain(crate::ChainError::InsufficientBalance).into(),
            wallet_chain_interact::Error::UtxoError(
                wallet_chain_interact::UtxoError::InsufficientFee,
            ) => crate::BusinessError::Chain(crate::ChainError::InsufficientFeeBalance).into(),
            _ => err.into(),
        }
    }

    // 转账金额不能小于对应币种精度的最小金额,并返回最小金额的u256表示
    pub fn check_min_transfer(
        value: &str,
        decimal: u8,
    ) -> Result<alloy::primitives::U256, crate::ServiceError> {
        let min = alloy::primitives::U256::from(1);
        let transfer_amount = unit::convert_to_u256(value, decimal)?;

        if transfer_amount < min {
            return Err(crate::BusinessError::Chain(
                crate::ChainError::AmountLessThanMin,
            ))?;
        }
        Ok(transfer_amount)
    }
}
