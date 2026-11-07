use super::account::{BalanceInfo, BalanceStr};
use crate::{
    domain::chain::swap::calc_slippage,
    request::transaction::{ApproveReq, DexRoute, QuoteReq},
    response_vo::EstimateFeeResp,
};
use alloy::primitives::U256;
use wallet_transport_backend::api::wallet::swap::ApproveInfo;

// 查询报价的响应
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiQuoteResp {
    pub chain_code: String,
    // 输入
    pub amount_in: BalanceStr,
    pub amount_in_symbol: String,
    pub amount_in_token_address: String,
    // 输出
    pub amount_out: BalanceStr,
    pub amount_out_symbol: String,
    pub amount_out_token_address: String,
    // 输入和输出的价值差
    pub price_spread: f64,
    // 预估的手续费
    pub fee: EstimateFeeResp,
    // 资源的消耗
    pub consumer: String,
    // 转换后的值
    pub from_token_price: f64,
    // 自动滑点值
    pub default_slippage: f64,
    // 滑点
    pub slippage: f64,
    // 最小获得数量
    pub min_amount: String,
    // 兑换路径
    pub dex_route_list: Vec<DexRoute>,
    // 需要授权的数量
    pub need_approve_amount: String,
    // 已经授权的数量
    pub approve_amount: String,
}

impl ApiQuoteResp {
    // native coin default slippage
    pub const DEFAULT_SLIPPAGE: f64 = 0.005;

    pub fn new_with_default_slippage(
        req: &QuoteReq,
        dex_route_list: Vec<DexRoute>,
        bal_in: BalanceStr,
        bal_out: BalanceStr,
    ) -> Self {
        let (rate, price_spread) = Self::calc_price_spread_and_rate(&bal_in, &bal_out);

        Self {
            chain_code: req.chain_code.clone(),
            amount_in: bal_in,
            amount_in_symbol: req.token_in.symbol.clone(),
            amount_in_token_address: req.token_in.token_addr.clone(),
            amount_out: bal_out,
            amount_out_symbol: req.token_out.symbol.clone(),
            amount_out_token_address: req.token_out.token_addr.clone(),
            price_spread,
            fee: EstimateFeeResp::default(),
            from_token_price: rate,
            slippage: ApiQuoteResp::DEFAULT_SLIPPAGE,
            default_slippage: ApiQuoteResp::DEFAULT_SLIPPAGE,
            min_amount: "0".to_string(),
            dex_route_list,
            consumer: "".to_string(),
            need_approve_amount: "0".to_string(),
            approve_amount: "0".to_string(),
        }
    }

    pub fn new(
        req: &QuoteReq,
        slippage: f64,
        default_slippage: f64,
        dex_route_list: Vec<DexRoute>,
        bal_in: BalanceStr,
        bal_out: BalanceStr,
    ) -> Self {
        let (rate, price_spread) = Self::calc_price_spread_and_rate(&bal_in, &bal_out);

        Self {
            chain_code: req.chain_code.clone(),
            amount_in: bal_in,
            amount_in_symbol: req.token_in.symbol.clone(),
            amount_in_token_address: req.token_in.token_addr.clone(),
            amount_out: bal_out,
            amount_out_symbol: req.token_out.symbol.clone(),
            amount_out_token_address: req.token_out.token_addr.clone(),
            price_spread,
            fee: EstimateFeeResp::default(),
            from_token_price: rate,
            slippage,
            default_slippage,
            min_amount: "0".to_string(),
            dex_route_list,
            consumer: "".to_string(),
            need_approve_amount: "0".to_string(),
            approve_amount: "0".to_string(),
        }
    }

    pub fn set_amount_out(&mut self, amount: U256, decimals: u32) {
        let min_amount = calc_slippage(amount, self.slippage);

        self.min_amount =
            wallet_utils::unit::format_to_string(min_amount, decimals as u8).unwrap_or_default();
    }

    // amount out 计算  滑点
    pub fn set_dex_amount_out(&mut self) -> Result<(), crate::error::service::ServiceError> {
        for dex_route in self.dex_route_list.iter_mut() {
            let amount = wallet_utils::unit::u256_from_str(&dex_route.amount_out)?;
            dex_route.amount_out = calc_slippage(amount, self.slippage).to_string();
        }
        Ok(())
    }

    // 计算token_in 和token_out的价差,以及兑换比例
    pub fn calc_price_spread_and_rate(
        amount_in: &BalanceStr,
        amount_out: &BalanceStr,
    ) -> (f64, f64) {
        let a_in = wallet_utils::unit::string_to_f64(&amount_in.amount).unwrap_or_default();

        let rate = if a_in > 0.0 {
            let a_out = wallet_utils::unit::string_to_f64(&amount_out.amount).unwrap_or_default();
            a_out / a_in
        } else {
            0.0
        };

        let spread = match (amount_in.fiat_value, amount_out.fiat_value) {
            (Some(in_val), Some(out_val)) if in_val > 0.0 && out_val > 0.0 => {
                ((out_val - in_val) / in_val) * 100.0
            }
            _ => 0.0,
        };

        (rate, spread)
    }
}

// 授权列表
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveList {
    pub chain_code: String,
    pub token_address: String,
    pub symbol: String,
    pub spender: String,
    pub from: String,
    pub amount: String,
    pub remaining_allowance: String,
}

impl From<ApproveInfo> for ApproveList {
    fn from(value: ApproveInfo) -> Self {
        let amount = if value.limit_type == ApproveReq::UN_LIMIT {
            "-1".to_string()
        } else {
            value.value.clone()
        };

        Self {
            chain_code: value.chain_code,
            token_address: value.token_addr,
            from: value.owner_address,
            symbol: "".to_string(),
            spender: value.spender,
            amount,
            remaining_allowance: value.value,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapTokenInfo {
    pub symbol: String,
    pub decimals: u32,
    pub chain_code: String,
    pub name: String,
    pub token_addr: String,
    pub balance: BalanceInfo,
}
