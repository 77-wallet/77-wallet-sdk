use crate::{
    domain::{
        app::config::ConfigDomain,
        assets::AssetsDomain,
        bill::BillDomain,
        chain::{
            adapter::{ChainAdapterFactory, TransactionAdapter, sol_tx::SYSTEM_ACCOUNT_RENT},
            transaction::ChainTransDomain,
        },
        coin::{CoinDomain, TokenCurrencyGetter},
        task_queue::TaskQueueDomain,
    },
    infrastructure::swap_client::{
        AggQuoteRequest, AggQuoteResp, DefaultQuoteResp, SolInstructResp, SolInstructionReq,
        SwapClient,
    },
    messaging::notify::{
        FrontendNotifyEvent,
        event::NotifyEvent,
        other::{Process, TransactionProcessFrontend},
    },
    request::transaction::{
        ApproveReq, DepositReq, DexRoute, QuoteReq, SwapInnerType, SwapReq, SwapTokenListReq,
        WithdrawReq, request_identity,
    },
    response_vo::{
        CommonFeeDetails, EstimateFeeResp,
        standard_wallet::{
            account::{BalanceInfo, BalanceStr},
            swap::{ApiQuoteResp, ApproveList, SwapTokenInfo},
        },
    },
};
use alloy::primitives::U256;
use rust_decimal::Decimal;
use std::time::{self};
use wallet_chain_interact::sol::SolFeeSetting;
use wallet_database::{
    CoreDbPool,
    entities::{
        asset_token_key::AssetTokenKey,
        assets::AssetsEntity,
        bill::{BillExtraSwap, BillKind, BillStatus, NewBillEntity},
        coin::CoinEntity,
    },
    pagination::Pagination,
    repositories::{
        account::AccountRepo, assets::AssetsRepo, bill::BillRepo, coin::CoinRepo,
        exchange_rate::ExchangeRateRepo,
    },
};
use wallet_transport_backend::{
    api::wallet::swap::{ApproveCancelReq, ApproveInfo, ApproveSaveParams, ChainDex},
    consts::endpoint::{SWAP_APPROVE_CANCEL, SWAP_APPROVE_SAVE},
    request::TokenQueryPriceReq,
};
use wallet_types::{chain::chain::ChainCode, constant::chain_code};
use wallet_utils::{
    address::AccountIndexMap,
    conversion,
    unit::{self, convert_to_u256, format_to_string},
};

pub struct SwapServer {
    ctx: &'static crate::context::Context,
    pub client: SwapClient,
}

impl SwapServer {
    pub fn new(
        ctx: &'static crate::context::Context,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let url = ctx.get_aggregate_api();
        let swap_client = SwapClient::new(&url);

        Ok(Self { ctx, client: swap_client? })
    }
}

impl SwapServer {
    pub async fn default_quote(
        &self,
        chain_code: String,
        token_in: String,
    ) -> Result<DefaultQuoteResp, crate::error::service::ServiceError> {
        let code = ChainCode::try_from(chain_code.as_str())?;
        let token_addr = CoinDomain::get_stable_coin_with_ctx(self.ctx, code).await?;

        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let stable_coin = CoinRepo::coin_by_chain_token_key(
            &chain_code,
            AssetTokenKey::from_raw(Some(token_addr.as_str())),
            &core_pool,
        )
        .await?;

        let (from_token, out_token) = if token_in.is_empty() {
            let token = CoinRepo::main_coin(&chain_code, &core_pool).await?;
            (token, stable_coin)
        } else if token_in == token_addr {
            // 传入的是稳定币
            let token = CoinRepo::main_coin(&chain_code, &core_pool).await?;

            (stable_coin, token)
        } else {
            let token = CoinRepo::coin_by_chain_token_key(
                &chain_code,
                AssetTokenKey::from_raw(Some(token_in.as_str())),
                &core_pool,
            )
            .await?;

            (token, stable_coin)
        };

        let res = DefaultQuoteResp {
            token_in: SwapTokenInfo {
                token_addr: from_token.token_address.as_db_str().to_string(),
                decimals: from_token.decimals as u32,
                symbol: from_token.symbol,
                chain_code: from_token.chain_code.to_string(),
                name: from_token.name,
                balance: BalanceInfo::default(),
            },
            token_out: SwapTokenInfo {
                token_addr: out_token.token_address.as_db_str().to_string(),
                decimals: out_token.decimals as u32,
                symbol: out_token.symbol,
                chain_code: out_token.chain_code.to_string(),
                name: out_token.name,
                balance: BalanceInfo::default(),
            },
        };

        Ok(res)
    }

    async fn get_bal_in_and_out(
        &self,
        req: &QuoteReq,
        amount_out: U256,
    ) -> Result<(BalanceStr, BalanceStr), crate::error::service::ServiceError> {
        // 查询两次后端
        let bal_in = TokenCurrencyGetter::get_bal_by_backend(
            self.ctx,
            &req.chain_code,
            &req.token_in.token_addr,
            &req.amount_in,
            req.token_in.decimals as u8,
        )
        .await?;

        let bal_out = TokenCurrencyGetter::get_bal_by_backend(
            self.ctx,
            &req.chain_code,
            &req.token_out.token_addr,
            &format_to_string(amount_out, req.token_out.decimals as u8)?,
            req.token_out.decimals as u8,
        )
        .await?;

        Ok((bal_in, bal_out))
    }

    pub async fn quote(
        &self,
        req: QuoteReq,
    ) -> Result<ApiQuoteResp, crate::error::service::ServiceError> {
        let chain_code = ChainCode::try_from(req.chain_code.as_str())?;

        let swap_inner_type =
            QuoteReq::swap_type(chain_code, &req.token_in.token_addr, &req.token_out.token_addr)?;

        match swap_inner_type {
            SwapInnerType::Withdraw => self.common_quote(req, SwapInnerType::Withdraw).await,
            SwapInnerType::Deposit => self.common_quote(req, SwapInnerType::Deposit).await,
            SwapInnerType::Swap => self.swap_quote(req).await,
        }
    }

    fn check_bal(&self, val: &str, bal: &str) -> Result<bool, crate::error::service::ServiceError> {
        Ok(conversion::decimal_from_str(val)? <= conversion::decimal_from_str(bal)?)
    }

    async fn token0_assets(
        &self,
        pool: &CoreDbPool,
        chain_code: &str,
        token_addr: &str,
        recipient: &str,
    ) -> Result<AssetsEntity, crate::error::service::ServiceError> {
        Ok(AssetsRepo::get_by_addr_token_opt(
            pool,
            chain_code,
            AssetTokenKey::from_raw(Some(token_addr)),
            recipient,
        )
        .await?
        .ok_or(crate::error::business::BusinessError::Assets(
            crate::error::business::assets::AssetsError::NotFoundAssets,
        ))?)
    }

    async fn common_quote(
        &self,
        req: QuoteReq,
        tx_type: SwapInnerType,
    ) -> Result<ApiQuoteResp, crate::error::service::ServiceError> {
        let amount_out =
            wallet_utils::unit::convert_to_u256(&req.amount_in, req.token_in.decimals as u8)?;

        let (bal_in, bal_out) = self.get_bal_in_and_out(&req, amount_out).await?;

        let dex_route_list = DexRoute::new(req.amount_in.clone(), &req.token_in, &req.token_out);
        let mut res =
            ApiQuoteResp::new_with_default_slippage(&req, vec![dex_route_list], bal_in, bal_out);
        res.set_amount_out(amount_out, req.token_out.decimals);

        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let assets = self
            .token0_assets(&core_pool, &req.chain_code, &req.token_in.token_addr, &req.recipient)
            .await?;

        if self.check_bal(&req.amount_in, &assets.balance)? {
            let adapter =
                ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &req.chain_code)
                    .await?;
            let main_coin = CoinRepo::main_coin(&req.chain_code, &core_pool).await?;

            let (consumer, content) = match tx_type {
                SwapInnerType::Withdraw => {
                    tracing::warn!("withdraw");
                    let params = WithdrawReq {
                        from: req.recipient.clone(),
                        token: req.token_in.token_addr.clone(),
                        amount: req.amount_in.clone(),
                        chain_code: req.chain_code.clone(),
                    };
                    adapter.withdraw_fee(self.ctx, params, &main_coin).await?
                }
                SwapInnerType::Deposit => {
                    tracing::warn!("deposit");
                    let params = DepositReq {
                        from: req.recipient.clone(),
                        token: req.token_out.token_addr.clone(),
                        amount: req.amount_in.clone(),
                        chain_code: req.chain_code.clone(),
                    };
                    adapter.deposit_fee(self.ctx, params, &main_coin).await?
                }
                SwapInnerType::Swap => {
                    return Err(crate::error::service::ServiceError::Parameter(
                        "不支持 Swap 类型用于 common_quote".into(),
                    ));
                }
            };

            let fee_resp = EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code, content);
            res.consumer = consumer;
            res.fee = fee_resp;
        }

        Ok(res)
    }

    // sol 手续费
    async fn check_bal_with_last_swap(
        &self,
        bal: &str,
        req: &QuoteReq,
        pool: &CoreDbPool,
        sol_fee: Option<f64>,
    ) -> Result<bool, crate::error::service::ServiceError> {
        // 尝试获取“需要扣减的金额”（如果条件不满足则为 None） 移除了最后一个交易所交易的钱
        let maybe_deduction = BillRepo::last_swap_bill(&req.recipient, &req.chain_code, pool)
            .await?
            .and_then(|bill| {
                wallet_utils::serde_func::serde_from_str::<BillExtraSwap>(&bill.extra)
                    .ok()
                    .filter(|extra| {
                        extra.from_token_address == req.token_in.token_addr
                            && extra.to_token_address == req.token_out.token_addr
                    })
                    .map(|_| bill.value) // 传出需扣减的字符串金额
            });

        let mut bal_ref = if let Some(pre) = maybe_deduction {
            let pre_amount = conversion::decimal_from_str(&pre)?;
            conversion::decimal_from_str(bal)? - pre_amount
        } else {
            conversion::decimal_from_str(bal)?
        };

        // 如果是sol 需要扣减手续费
        if req.is_sol() {
            let fee = sol_fee.unwrap_or_default() + SYSTEM_ACCOUNT_RENT;
            let fee = Decimal::from_f64_retain(fee).unwrap();

            if req.token_in.token_addr.is_empty() {
                bal_ref -= fee
            } else {
                // 验证主币的金额是否足够 支付手续费
                let assets = AssetsRepo::get_by_addr_token(
                    pool,
                    &req.chain_code,
                    AssetTokenKey::Native,
                    &req.recipient,
                )
                .await?;
                if !self.check_bal(&fee.to_string(), &assets.balance)? {
                    return Ok(false);
                }
            }
        }
        self.check_bal(&req.amount_in, &bal_ref.to_string())
    }

    // 是否进行模拟交易 true 需要进行模拟
    async fn whether_simulate(
        &self,
        req: &QuoteReq,
        resp: &mut ApiQuoteResp,
        adapter: &TransactionAdapter,
        sol_fee: Option<&SolFeeSetting>,
    ) -> Result<bool, crate::error::service::ServiceError> {
        // 非主币 且 不是sol ,优先考虑授权的数量
        if !req.token_in.token_addr.is_empty() {
            let allowance = adapter
                .allowance(&req.recipient, &req.token_in.token_addr, &req.aggregator_addr)
                .await?;

            let amount_in = convert_to_u256(&req.amount_in, req.token_in.decimals as u8)?;

            // -1 代表着无线的授权 写入授权的结果
            if allowance > U256::MAX >> 1 {
                resp.approve_amount = "-1".to_string();
            } else {
                resp.approve_amount = format_to_string(allowance, req.token_in.decimals as u8)?;
            }

            if amount_in > allowance {
                let diff = amount_in - allowance;
                resp.need_approve_amount = format_to_string(diff, req.token_in.decimals as u8)?;
                return Ok(false);
            }
        }

        // 判断余额是否足 进行模拟
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let assets = self
            .token0_assets(&core_pool, &req.chain_code, &req.token_in.token_addr, &req.recipient)
            .await?;

        let sol_fee = sol_fee.map(|f| f.transaction_fee());

        self.check_bal_with_last_swap(&assets.balance, req, &core_pool, sol_fee).await
    }

    async fn handle_sol_fee(
        &self,
        fee_setting: &SolFeeSetting,
        resp: &mut ApiQuoteResp,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let main_coin = CoinRepo::main_coin(&resp.chain_code, &core_pool).await?;

        let fee = fee_setting.transaction_fee();

        let currency = {
            let currency = crate::app_state::APP_STATE.read().await;
            currency.currency().to_string()
        };

        let token_currency = TokenCurrencyGetter::get_currency_by_token_key(
            self.ctx,
            &currency,
            &resp.chain_code,
            &main_coin.symbol,
            AssetTokenKey::Native,
        )
        .await?;
        let content = CommonFeeDetails::new(fee, token_currency, &currency)?.to_json_str()?;
        let fee_resp = EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code, content);

        let consumer = wallet_utils::serde_func::serde_to_string(&fee_setting)?;

        resp.consumer = consumer;
        resp.fee = fee_resp;

        Ok(())
    }

    async fn swap_quote(
        &self,
        req: QuoteReq,
    ) -> Result<ApiQuoteResp, crate::error::service::ServiceError> {
        // 查询后端,获取报价(调用合约查路径)
        let params = AggQuoteRequest::try_from(&req)?;

        let instance = time::Instant::now();
        let quote_resp = self.client.get_quote(params).await?;
        tracing::warn!("quote time: {}", instance.elapsed().as_secs_f64());

        let amount_out = unit::u256_from_str(&quote_resp.amount_out)?;
        let (bal_in, bal_out) = self.get_bal_in_and_out(&req, amount_out).await?;

        // 获取滑点
        let default_slippage = quote_resp.get_slippage();
        let slippage = req.get_slippage(default_slippage)?;

        // 初始化响应
        let mut res = ApiQuoteResp::new(
            &req,
            slippage,
            default_slippage,
            quote_resp.dex_route_list.clone(),
            bal_in,
            bal_out,
        );

        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &req.chain_code)
                .await?;

        // sol 单独获取手续费
        let sol_fee = if req.is_sol() {
            let fee_setting = adapter.sol_swap_fee(&req, None).await?;

            self.handle_sol_fee(&fee_setting, &mut res).await?;

            Some(fee_setting)
        } else {
            None
        };

        // 先使用报价返回的amount_out,如果可以进行模拟，那么后续使用模拟的值覆盖
        res.set_amount_out(amount_out, req.token_out.decimals);
        res.set_dex_amount_out()?;

        // 是否需要进行模拟
        if self.whether_simulate(&req, &mut res, &adapter, sol_fee.as_ref()).await? {
            self.simulate_and_fill(&req, &quote_resp, &mut res, &adapter).await?;
        };

        Ok(res)
    }

    pub async fn sol_instructions(
        &self,
        payer: &str,
        chain_code: &str,
        is_native_token: bool,
        amount_in: String,
        amount_out: String,
        dex_route_list: Vec<DexRoute>,
        is_simulate: bool,
    ) -> Result<Option<SolInstructResp>, crate::error::service::ServiceError> {
        if chain_code == chain_code::SOLANA {
            let req = SolInstructionReq {
                unique: request_identity(payer),
                payer: payer.to_string(),
                is_native_token,
                amount_in,
                amount_out,
                dex_route_list,
                is_simulate,
            };

            // tracing::warn!(
            //     "get instruction params: {}",
            //     wallet_utils::serde_func::serde_to_string(&req).unwrap()
            // );
            // let _res = FrontendNotifyEvent::send_debug(&req).await;

            let instance = time::Instant::now();
            let res = self.client.sol_instructions(req).await?;
            tracing::warn!("get instruction time: {}", instance.elapsed().as_secs_f64());
            Ok(Some(res))
        } else {
            Ok(None)
        }
    }

    // 模拟交易结果，并录入手续费
    // 其他链 手续费和模拟绑定在一起的，sol 目前是独立出来的，sol如果手续费不够，会模拟失败。
    async fn simulate_and_fill(
        &self,
        req: &QuoteReq,
        quote_resp: &AggQuoteResp,
        res: &mut ApiQuoteResp,
        adapter: &TransactionAdapter,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let main_coin = CoinRepo::main_coin(&req.chain_code, &core_pool).await?;

        // sol获取交易指令
        let instructions = self
            .sol_instructions(
                &req.recipient,
                &req.chain_code,
                req.is_native(),
                quote_resp.amount_in.clone(),
                0.to_string(),
                quote_resp.dex_route_list.clone(),
                true,
            )
            .await?;

        let instance = time::Instant::now();
        // 模拟报价(consumer 资源的消耗，，content 费用的具体内容)
        let backend_api = self.ctx.get_global_backend_api();
        let (amount_out, consumer, content) = adapter
            .swap_quote_with_ctx(
                self.ctx,
                req,
                quote_resp,
                &main_coin.symbol,
                instructions,
                backend_api.as_ref(),
            )
            .await?;

        // 最终模拟交易能够获取多少 amount_out
        res.set_amount_out(amount_out, req.token_out.decimals);

        // 手续费的设置
        let fee_resp = EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code, content);
        if !req.is_sol() {
            res.consumer = consumer;
            res.fee = fee_resp;
        }

        tracing::warn!("simulate time: {}", instance.elapsed().as_secs_f64());

        Ok(())
    }

    // 执行兑换
    pub async fn swap(
        &self,
        req: SwapReq,
        fee: String,
        password: String,
    ) -> Result<String, crate::error::service::ServiceError> {
        // 构建事件
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            wallet_database::entities::bill::BillKind::Swap,
            Process::Building,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;
        let key =
            ChainTransDomain::get_key(self.ctx, &req.recipient, &req.chain_code, &password, &None)
                .await?;

        // 查询余额是否足够
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let token_in = AssetsRepo::get_by_addr_token(
            &core_pool,
            &req.chain_code,
            AssetTokenKey::from_raw(Some(req.token_in.token_addr.as_str())),
            &req.recipient,
        )
        .await?;
        if !self.check_bal(&req.amount_in, &token_in.balance)? {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(req.recipient.clone())
                            .chain_code(req.chain_code.clone())
                            .token_addr(req.token_in.token_addr.clone())
                            .value(req.amount_in.clone())
                            .balance(token_in.balance.clone())
                            .need(req.amount_in.clone())
                            .reason("token balance is insufficient for swap"),
                    ),
                ),
            ));
        }

        // 广播事件
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            wallet_database::entities::bill::BillKind::Swap,
            Process::Broadcast,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;
        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &req.chain_code)
                .await?;

        let chain_code = ChainCode::try_from(req.chain_code.as_str())?;
        let swap_inner_type =
            QuoteReq::swap_type(chain_code, &req.token_in.token_addr, &req.token_out.token_addr)?;

        let resp = match swap_inner_type {
            SwapInnerType::Deposit => {
                let params = DepositReq {
                    from: req.recipient.clone(),
                    token: req.token_out.token_addr.clone(),
                    amount: req.amount_in.clone(),
                    chain_code: req.chain_code.clone(),
                };
                let value = wallet_utils::unit::convert_to_u256(
                    &req.amount_in,
                    req.token_in.decimals as u8,
                )?;
                adapter.deposit(&params, fee, key, value).await?
            }
            SwapInnerType::Withdraw => {
                let params = WithdrawReq {
                    from: req.recipient.clone(),
                    token: req.token_in.token_addr.clone(),
                    amount: req.amount_in.clone(),
                    chain_code: req.chain_code.clone(),
                };
                let value = wallet_utils::unit::convert_to_u256(
                    &req.amount_in,
                    req.token_in.decimals as u8,
                )?;

                adapter.withdraw(&params, fee, key, value).await?
            }
            SwapInnerType::Swap => {
                let amount_in = wallet_utils::unit::convert_to_u256(
                    &req.amount_in,
                    req.token_in.decimals as u8,
                )?
                .to_string();

                let amount_out = wallet_utils::unit::convert_to_u256(
                    &req.min_amount_out,
                    req.token_out.decimals as u8,
                )?
                .to_string();

                let instructions = self
                    .sol_instructions(
                        &req.recipient,
                        &req.chain_code,
                        req.is_native_token(),
                        amount_in,
                        amount_out,
                        req.dex_router.clone(),
                        false,
                    )
                    .await?;
                adapter.swap(&req, fee, key, instructions).await?
            }
        };

        //  if token_out if new assets add it
        let token_out = req.token_out.clone();
        let out_assets = AssetsRepo::get_by_addr_token_opt(
            &core_pool,
            &req.chain_code,
            AssetTokenKey::from_raw(Some(token_out.token_addr.as_str())),
            &req.recipient,
        )
        .await?;
        if out_assets.is_none() {
            AssetsDomain::swap_sync_assets(
                self.ctx,
                token_out,
                req.recipient.clone(),
                req.chain_code.clone(),
            )
            .await?;
        }

        // 写入本地交易记录表
        let mut new_bill: NewBillEntity<BillExtraSwap> = BillRepo::try_build_bill_from(req)?;
        new_bill.hash = resp.tx_hash.clone();
        new_bill.resource_consume = resp.resource_consume()?;
        new_bill.transaction_fee = resp.fee;
        let pool = self.ctx.core_pool()?;
        BillDomain::create_bill(&pool, new_bill).await?;

        Ok(resp.tx_hash)
    }

    pub async fn token_list(
        &self,
        req: SwapTokenListReq,
    ) -> Result<Pagination<SwapTokenInfo>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let chain_code = (!req.chain_code.is_empty()).then(|| req.chain_code.clone());

        let chain_codes = chain_code.into_iter().collect::<Vec<_>>();
        let list = AccountRepo::account_list_by_wallet_address_and_account_id_and_chain_codes(
            core_pool.clone(),
            Some(&req.wallet_address),
            Some(req.account_id as u32),
            chain_codes,
        )
        .await?;
        let address = list.iter().map(|x| x.address.clone()).collect::<Vec<String>>();

        let coins = CoinRepo::coin_list_with_assets(
            &req.search,
            req.exclude_token,
            req.chain_code.to_string(),
            address,
            req.page_num,
            req.page_size,
            core_pool.clone(),
        )
        .await?;

        let mut resp = Pagination::<SwapTokenInfo> {
            page: coins.page,
            page_size: coins.page_size,
            total_count: coins.total_count,
            data: vec![],
        };

        let currency = {
            let state = crate::app_state::APP_STATE.read().await;
            state.currency().to_string()
        };
        let exchange = ExchangeRateRepo::exchange_rate(&currency, core_pool.clone()).await?;

        let mut req = TokenQueryPriceReq(Vec::new());
        for coin in coins.data {
            // 查询币价的请求参数
            let contract_address = coin.token_address.clone();
            req.insert(&coin.chain_code, &contract_address);

            let balance = if coin.balance != "0" {
                let unit_price = unit::string_to_f64(&coin.price)? * exchange.rate;

                let amount = unit::string_to_f64(&coin.balance)?;
                BalanceInfo::new(amount, Some(unit_price), &currency)
            } else {
                BalanceInfo::default()
            };

            let token_info = SwapTokenInfo {
                symbol: coin.symbol,
                decimals: coin.decimals,
                token_addr: coin.token_address,
                name: coin.name,
                chain_code: coin.chain_code,
                balance,
            };
            resp.data.push(token_info);
        }

        let backend_api = self.ctx.get_global_backend_api();
        let tokens = backend_api.token_query_price(&req).await?.list;
        for token in tokens {
            CoinRepo::update_price_unit1(
                &token.chain_code,
                wallet_database::entities::asset_token_key::AssetTokenKey::from_raw(
                    token.token_address.as_deref(),
                ),
                &token.price.to_string(),
                &core_pool,
            )
            .await?;
        }

        Ok(resp)
    }

    pub async fn chain_list(&self) -> Result<Vec<ChainDex>, crate::error::service::ServiceError> {
        let backend_api = self.ctx.get_global_backend_api();
        let version = ConfigDomain::get_app_version(self.ctx).await?.app_version;
        let result = backend_api.support_chain_list_v2(version).await?;

        Ok(result.support_chain)
    }

    pub async fn approve_fee(
        &self,
        req: ApproveReq,
        is_cancel: bool,
    ) -> Result<EstimateFeeResp, crate::error::service::ServiceError> {
        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &req.chain_code)
                .await?;

        let (value, tx_kind) = if is_cancel {
            (alloy::primitives::U256::ZERO, BillKind::UnApprove)
        } else {
            (alloy::primitives::U256::MAX, BillKind::Approve)
        };

        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let main_coin = CoinRepo::main_coin(&req.chain_code, &core_pool).await?;

        // 验证是否有存在的approve
        let last_bill = BillRepo::last_approve_bill(
            &req.from,
            &req.spender,
            &req.contract,
            &req.chain_code,
            tx_kind,
            &core_pool,
        )
        .await?;
        if last_bill.is_some() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::ApproveRepeated,
                ),
            ));
        }

        let fee = adapter.approve_fee(self.ctx, &req, value, &main_coin.symbol).await?;

        let fee_resp = EstimateFeeResp::new(main_coin.symbol, main_coin.chain_code.clone(), fee);
        Ok(fee_resp)
    }

    pub async fn approve(
        &self,
        req: ApproveReq,
        password: String,
    ) -> Result<String, crate::error::service::ServiceError> {
        // get coin
        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let coin = CoinRepo::coin_by_chain_token_key(
            &req.chain_code,
            AssetTokenKey::from_raw(Some(req.contract.as_str())),
            &core_pool,
        )
        .await?;

        // 构建交易事件
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            wallet_database::entities::bill::BillKind::Approve,
            Process::Building,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;

        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &req.chain_code)
                .await?;

        // 本地数据库中是否有授权的交易
        let last_bill = BillRepo::last_approve_bill(
            &req.from,
            &req.spender,
            &req.contract,
            &req.chain_code,
            BillKind::Approve,
            &core_pool,
        )
        .await?;
        if last_bill.is_some() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::ApproveRepeated,
                ),
            ));
        }

        // check already approved
        let allowance = adapter.allowance(&req.from, &req.contract, &req.spender).await?;
        if allowance > alloy::primitives::U256::ZERO {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::ApproveRepeated,
                ),
            ));
        }

        // check balance
        let token_in =
            self.token0_assets(&core_pool, &req.chain_code, &req.contract, &req.from).await?;
        if !self.check_bal(&req.value, &token_in.balance)? {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(req.from.clone())
                            .chain_code(req.chain_code.clone())
                            .token_addr(req.contract.clone())
                            .value(req.value.clone())
                            .balance(token_in.balance.clone())
                            .need(req.value.clone())
                            .reason("token balance is insufficient for approve"),
                    ),
                ),
            ));
        }

        let private_key =
            ChainTransDomain::get_key(self.ctx, &req.from, &req.chain_code, &password, &None)
                .await?;

        // 广播交易事件
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            wallet_database::entities::bill::BillKind::Approve,
            Process::Broadcast,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;

        let value = req.get_value(coin.decimals)?;
        let resp = adapter.approve(&req, private_key, value).await?;

        let core_pool = self.ctx.core_pool()?;
        let account =
            AccountRepo::account_with_wallet(&req.from, &req.chain_code, core_pool).await?;

        // 上报后端
        let backend_req = ApproveSaveParams::new(
            account.get_index()?,
            &account.uid,
            &req.chain_code,
            &req.spender,
            &req.from,
            &req.contract,
            req.value.clone(),
            &resp.tx_hash.clone(),
            req.get_approve_type(),
        );
        TaskQueueDomain::send_or_to_queue_with_ctx(self.ctx, backend_req, SWAP_APPROVE_SAVE)
            .await?;

        // 写入本地交易
        let mut new_bill: NewBillEntity = BillRepo::try_build_bill_from(req)?;
        new_bill.hash = resp.tx_hash.clone();
        new_bill.symbol = coin.symbol;
        new_bill.resource_consume = resp.resource_consume()?;
        new_bill.transaction_fee = resp.fee;
        let pool = self.ctx.core_pool()?;
        BillDomain::create_bill(&pool, new_bill).await?;

        Ok(resp.tx_hash)
    }

    pub async fn approve_list(
        &self,
        uid: String,
        account_id: u32,
    ) -> Result<Vec<ApproveList>, crate::error::service::ServiceError> {
        let index_map = AccountIndexMap::from_account_id(account_id)?;

        let backend = self.ctx.get_global_backend_api();
        let resp = backend.approve_list(uid, index_map.input_index).await?;

        let mut res = vec![];

        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let mut used_ids = vec![];
        for item in resp.list.into_iter() {
            let coin = CoinRepo::coin_by_chain_token_key(
                &item.chain_code,
                AssetTokenKey::from_raw(Some(item.token_addr.as_str())),
                &core_pool,
            )
            .await?;
            if item.limit_type == ApproveReq::UN_LIMIT {
                // 无限授权的类型
                let mut approve_info = ApproveList::from(item);
                approve_info.symbol = coin.symbol;
                res.push(approve_info)
            } else {
                // 获取allowance 情况
                self.push_approve_if_nonzero(item, &coin, &core_pool, &mut res, &mut used_ids)
                    .await?;
            }
        }

        // 通知后端哪些已经被使用
        if !used_ids.is_empty() {
            backend.update_used_approve(used_ids).await?;
        }

        Ok(res)
    }

    async fn push_approve_if_nonzero(
        &self,
        item: ApproveInfo,
        coin: &CoinEntity,
        pool: &CoreDbPool,
        res: &mut Vec<ApproveList>,
        used_ids: &mut Vec<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 1) 只有在 bill 不存在，或者存在且状态为 Success 时才继续
        if let Some(bill) = BillRepo::get_by_hash_opt(&item.hash, pool).await? {
            if bill.status != BillStatus::Success.to_i8() {
                return Ok(()); // 非成功直接跳过，不打链上请求
            }
        }

        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &item.chain_code)
                .await?;
        let allowance =
            adapter.allowance(&item.owner_address, &item.token_addr, &item.spender).await?;

        // 3) allowance 为 0：标记可丢弃
        if allowance.is_zero() {
            used_ids.push(item.id);
            return Ok(());
        }

        // 4) 组装显示数据
        let unit = coin.decimals;
        let origin_allowance = wallet_utils::unit::convert_to_u256(&item.value, unit)?;

        let mut approve_info = ApproveList::from(item);
        approve_info.amount = wallet_utils::unit::format_to_string(origin_allowance, unit)?;

        approve_info.remaining_allowance = wallet_utils::unit::format_to_string(allowance, unit)?;
        approve_info.symbol = coin.symbol.clone();

        res.push(approve_info);
        Ok(())
    }

    pub async fn approve_cancel(
        &self,
        req: ApproveReq,
        password: String,
    ) -> Result<String, crate::error::service::ServiceError> {
        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            wallet_database::entities::bill::BillKind::UnApprove,
            Process::Building,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;

        let private_key =
            ChainTransDomain::get_key(self.ctx, &req.from, &req.chain_code, &password, &None)
                .await?;
        let adapter =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(&self.ctx, &req.chain_code)
                .await?;

        let pool = self.ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let coin = CoinRepo::coin_by_chain_token_key(
            &req.chain_code,
            AssetTokenKey::from_raw(Some(req.contract.as_str())),
            &core_pool,
        )
        .await?;

        // 本地数据库中是否有授权的交易
        let last_bill = BillRepo::last_approve_bill(
            &req.from,
            &req.spender,
            &req.contract,
            &req.chain_code,
            BillKind::UnApprove,
            &core_pool,
        )
        .await?;
        if last_bill.is_some() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::ApproveCanceling,
                ),
            ));
        }

        let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new(
            wallet_database::entities::bill::BillKind::Approve,
            Process::Broadcast,
        ));
        FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;
        let value = alloy::primitives::U256::ZERO;
        let resp = adapter.approve(&req, private_key, value).await?;

        let backend = ApproveCancelReq {
            spender: req.spender.clone(),
            token_addr: req.contract.clone(),
            owner_address: req.from.clone(),
            tx_hash: resp.tx_hash.clone(),
        };
        TaskQueueDomain::send_or_to_queue_with_ctx(self.ctx, backend, SWAP_APPROVE_CANCEL).await?;
        // 写入本地交易
        let mut new_bill: NewBillEntity = BillRepo::try_build_bill_from(req)?;
        new_bill.hash = resp.tx_hash.clone();
        new_bill.symbol = coin.symbol;
        new_bill.tx_kind = wallet_database::entities::bill::BillKind::UnApprove;
        new_bill.resource_consume = resp.resource_consume()?;
        new_bill.transaction_fee = resp.fee;
        let pool = self.ctx.core_pool()?;
        BillDomain::create_bill(&pool, new_bill).await?;

        Ok(resp.tx_hash)
    }
}
