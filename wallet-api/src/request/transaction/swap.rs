use crate::{
    domain::chain::swap::{evm_swap::SwapParams, get_warp_address},
    infrastructure::swap_client::{AggQuoteRequest, DexId},
};
use alloy::primitives::U256;
use rand::Rng as _;
use wallet_database::{
    entities::{
        asset_token_key::AssetTokenKey,
        bill::{BillExtraSwap, BillKind, NewBillEntity},
    },
    repositories::bill::BillRepo,
};
use wallet_types::{chain::chain::ChainCode, constant::chain_code};

pub fn request_identity(user: &str) -> String {
    let mut rng = rand::thread_rng();
    let n: u32 = rng.gen_range(0..1000);

    format!("{}_{}_{}", user, wallet_utils::time::now().timestamp(), n)
}

#[derive(Debug, serde::Serialize)]
pub struct SwapTokenListReq {
    pub chain_code: String,
    pub search: String,
    pub exclude_token: Vec<String>,
    pub wallet_address: String,
    pub account_id: i32,
    pub page_num: i64,
    pub page_size: i64,
}

impl From<SwapTokenListReq> for wallet_transport_backend::request::SwapTokenQueryReq {
    fn from(value: SwapTokenListReq) -> Self {
        Self {
            chain_code: value.chain_code,
            search: value.search,
            page_num: value.page_num,
            page_size: value.page_size,
            exclude_tokens: value.exclude_token,
        }
    }
}

// deposit,和withdraw 统一认为swap
#[derive(Debug)]
pub enum SwapInnerType {
    Swap,
    Deposit,
    Withdraw,
}

pub struct DepositReq {
    pub from: String,
    pub amount: String,
    pub token: String,
    pub chain_code: String,
}

#[derive(Debug)]
pub struct WithdrawReq {
    pub from: String,
    pub amount: String,
    pub token: String,
    pub chain_code: String,
}

#[derive(Debug)]
pub struct ApproveReq {
    pub contract: String,
    pub from: String,
    pub spender: String,
    pub value: String,
    pub chain_code: String,
}
impl ApproveReq {
    pub const NORMAL: &'static str = "NORMAL";
    pub const UN_LIMIT: &'static str = "UN_LIMIT";

    pub fn get_approve_type(&self) -> &'static str {
        if self.value == "-1" || self.value == "0" { Self::UN_LIMIT } else { Self::NORMAL }
    }

    pub fn get_value(&self, decimals: u8) -> Result<U256, crate::error::service::ServiceError> {
        if self.value == "-1" || self.value == "0" {
            Ok(U256::MAX)
        } else {
            Ok(wallet_utils::unit::convert_to_u256(&self.value, decimals)?)
        }
    }
}

#[derive(Debug)]
pub struct ApproveCancel {
    pub contract: String,
    pub from: String,
    pub spender: String,
    pub chain_code: String,
}

impl TryFrom<ApproveReq> for NewBillEntity {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: ApproveReq) -> Result<Self, Self::Error> {
        let amount = wallet_utils::unit::string_to_f64(&value.value)?;
        if !amount.is_finite() {
            return Err(
                wallet_utils::Error::Other("approve amount must be finite".to_string()).into()
            );
        }

        let mut bill = BillRepo::build_bill(
            "".to_string(),
            value.from,
            value.spender,
            amount,
            value.chain_code,
            "".to_string(),
            false,
            BillKind::Approve,
            "".to_string(),
        );
        bill.tx_type = 1;
        bill.token = AssetTokenKey::from(value.contract);
        Ok(bill)
    }
}

#[derive(Debug)]
pub struct SwapReq {
    pub aggregator_addr: String,
    // 链code
    pub chain_code: String,
    // 输出金额
    pub amount_in: String,
    // 输出金额
    pub amount_out: String,
    // 接受最小的输出金额
    pub min_amount_out: String,
    // 接收地址
    pub recipient: String,
    // 输入token
    pub token_in: SwapTokenInfo,
    // 输出token
    pub token_out: SwapTokenInfo,
    // 路由数据
    pub dex_router: Vec<DexRoute>,
    // 允许部分兑换
    pub allow_partial_fill: bool,
}

impl SwapReq {
    pub fn is_native_token(&self) -> bool {
        self.token_in.token_addr.is_empty() || self.token_out.token_addr.is_empty()
    }
}

impl TryFrom<&SwapReq> for SwapParams {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &SwapReq) -> Result<Self, Self::Error> {
        let amount_in =
            wallet_utils::unit::convert_to_u256(&value.amount_in, value.token_in.decimals as u8)?;

        let min_amount_out = wallet_utils::unit::convert_to_u256(
            &value.min_amount_out,
            value.token_out.decimals as u8,
        )?;

        if value.chain_code == chain_code::ETHEREUM || value.chain_code == chain_code::BNB {
            Ok(SwapParams {
                aggregator_addr: wallet_utils::address::parse_eth_address(&value.aggregator_addr)?,
                amount_in,
                min_amount_out,
                recipient: wallet_utils::address::parse_eth_address(&value.recipient)?,
                dex_router: value.dex_router.clone(),
                token_in: SwapParams::eth_parse_or_zero_addr(&value.token_in.token_addr)?,
                token_out: SwapParams::eth_parse_or_zero_addr(&value.token_out.token_addr)?,
                allow_partial_fill: value.allow_partial_fill,
            })
        } else {
            Ok(SwapParams {
                aggregator_addr: QuoteReq::addr_tron_to_eth(&value.aggregator_addr)?,
                amount_in,
                min_amount_out,
                recipient: QuoteReq::addr_tron_to_eth(&value.recipient)?,
                dex_router: value.dex_router.clone(),
                token_in: SwapParams::tron_parse_or_zero_addr(&value.token_in.token_addr)?,
                token_out: SwapParams::tron_parse_or_zero_addr(&value.token_out.token_addr)?,
                allow_partial_fill: value.allow_partial_fill,
            })
        }
    }
}

impl TryFrom<SwapReq> for NewBillEntity<BillExtraSwap> {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: SwapReq) -> Result<Self, Self::Error> {
        let amount_in = wallet_utils::unit::string_to_f64(&value.amount_in)?;
        let amount_out = wallet_utils::unit::string_to_f64(&value.amount_out)?;

        let extra = BillExtraSwap {
            from_token_symbol: value.token_in.symbol.clone(),
            from_token_address: value.token_in.token_addr.clone(),
            from_token_amount: amount_in,
            to_token_symbol: value.token_out.symbol.clone(),
            to_token_address: value.token_out.token_addr.clone(),
            to_token_amount: amount_out,
            price: amount_out / amount_in,
        };

        let mut bill = BillRepo::build_bill_with_extra::<BillExtraSwap>(
            "".to_string(),
            value.recipient,
            "".to_string(),
            wallet_utils::unit::string_to_f64(&value.amount_in)?,
            value.chain_code,
            value.token_in.symbol,
            false,
            BillKind::Swap,
            "".to_string(),
        );
        bill.extra = Some(extra);
        Ok(bill)
    }
}

// 前端请求报价的参数
pub struct QuoteReq {
    // 聚合器提供的地址
    pub aggregator_addr: String,
    pub recipient: String,
    pub chain_code: String,
    // 未处理精度的值
    pub amount_in: String,
    pub token_in: SwapTokenInfo,
    pub token_out: SwapTokenInfo,
    // 选择的池子
    pub dex_list: Vec<i32>,
    // 滑点
    pub slippage: Option<f64>,
    // 允许部分兑换
    pub allow_partial_fill: bool,
}
impl QuoteReq {
    pub fn eth_or_weth(
        address: &str,
        chain_code: ChainCode,
    ) -> Result<String, crate::error::service::ServiceError> {
        if address.is_empty() {
            get_warp_address(chain_code).map(|x| x.to_string())
        } else {
            Ok(address.to_string())
        }
    }

    pub fn is_sol(&self) -> bool {
        self.chain_code == chain_code::SOLANA
    }

    pub fn amount_in_u256(&self) -> Result<U256, crate::error::service::ServiceError> {
        Ok(wallet_utils::unit::convert_to_u256(&self.amount_in, self.token_in.decimals as u8)?)
    }

    pub fn recipient_address(
        &self,
    ) -> Result<alloy::primitives::Address, crate::error::service::ServiceError> {
        Ok(wallet_utils::address::parse_eth_address(&self.recipient)?)
    }

    pub fn aggregator_address(
        &self,
    ) -> Result<alloy::primitives::Address, crate::error::service::ServiceError> {
        Ok(wallet_utils::address::parse_eth_address(&self.aggregator_addr)?)
    }

    // 波场的地址 转eth alloy Address type
    pub fn addr_tron_to_eth(
        addr: &str,
    ) -> Result<alloy::primitives::Address, crate::error::service::ServiceError> {
        let hex_addr = wallet_utils::address::bs58_addr_to_hex_bytes(addr)?;

        let hex_addr = hex::encode(&hex_addr[1..21]);

        Ok(wallet_utils::address::parse_eth_address(&hex_addr)?)
    }

    // 根据token地址判断是 进行那种swap交易
    pub fn swap_type(
        chain_code: ChainCode,
        token_in: &str,
        token_out: &str,
    ) -> Result<SwapInnerType, crate::error::service::ServiceError> {
        let warp_coin = get_warp_address(chain_code)?;

        if token_in.is_empty() && token_out == warp_coin {
            Ok(SwapInnerType::Deposit)
        } else if token_in == warp_coin && token_out.is_empty() {
            Ok(SwapInnerType::Withdraw)
        } else {
            Ok(SwapInnerType::Swap)
        }
    }

    // 前端有传入的使用前端传入的，没有使用报价返回的。
    pub fn get_slippage(
        &self,
        default_slippage: f64,
    ) -> Result<f64, crate::error::service::ServiceError> {
        let slippage = self.slippage.unwrap_or(default_slippage);
        if !slippage.is_finite() || !(0.0..1.0).contains(&slippage) {
            return Err(crate::error::service::ServiceError::Parameter(format!(
                "slippage must be finite and in the range [0, 1), got {slippage}"
            )));
        }
        Ok(slippage)
    }

    // 是否是进行主币之间的兑换
    pub fn is_native(&self) -> bool {
        self.token_in.token_addr.is_empty() || self.token_out.token_addr.is_empty()
    }
}

#[cfg(test)]
mod slippage_tests {
    use super::{QuoteReq, SwapTokenInfo};

    fn quote(slippage: Option<f64>) -> QuoteReq {
        QuoteReq {
            aggregator_addr: String::new(),
            recipient: String::new(),
            chain_code: String::new(),
            amount_in: "1".to_string(),
            token_in: SwapTokenInfo {
                symbol: "IN".to_string(),
                decimals: 6,
                token_addr: String::new(),
            },
            token_out: SwapTokenInfo {
                symbol: "OUT".to_string(),
                decimals: 6,
                token_addr: String::new(),
            },
            dex_list: Vec::new(),
            slippage,
            allow_partial_fill: false,
        }
    }

    #[test]
    fn get_slippage_accepts_valid_custom_and_default_values() {
        assert_eq!(quote(Some(0.0)).get_slippage(0.01).unwrap(), 0.0);
        assert_eq!(quote(Some(0.5)).get_slippage(0.01).unwrap(), 0.5);
        assert_eq!(quote(Some(0.9999)).get_slippage(0.01).unwrap(), 0.9999);
        assert_eq!(quote(None).get_slippage(0.005).unwrap(), 0.005);
    }

    #[test]
    fn get_slippage_rejects_invalid_custom_values() {
        for slippage in [-0.01, 1.0, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(quote(Some(slippage)).get_slippage(0.01).is_err(), "accepted {slippage}");
        }
    }

    #[test]
    fn get_slippage_rejects_invalid_backend_default() {
        assert!(quote(None).get_slippage(1.0).is_err());
        assert!(quote(None).get_slippage(f64::INFINITY).is_err());
    }
}

#[derive(Clone, Debug)]
pub struct SwapTokenInfo {
    pub symbol: String,
    pub decimals: u32,
    pub token_addr: String,
}

impl TryFrom<&QuoteReq> for AggQuoteRequest {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &QuoteReq) -> Result<Self, Self::Error> {
        let chain_code = ChainCode::try_from(value.chain_code.as_str())?;

        let amount =
            wallet_utils::unit::convert_to_u256(&value.amount_in, value.token_in.decimals as u8)?;

        let unique = request_identity(value.recipient.as_str());

        let dex_ids = value.dex_list.iter().map(|dex_id| DexId { dex_id: *dex_id }).collect();

        Ok(Self {
            chain_code: chain_code.to_string(),
            amount: amount.to_string(),
            unique,
            in_token_addr: QuoteReq::eth_or_weth(&value.token_in.token_addr, chain_code)?,
            out_token_addr: QuoteReq::eth_or_weth(&value.token_out.token_addr, chain_code)?,
            dex_ids,
        })
    }
}

// 路由
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DexRoute {
    pub amount_in: String,
    pub amount_out: String,
    pub percentage: String,
    pub route_in_dex: Vec<RouteInDex>,
}

impl DexRoute {
    // 针对 deposit 和 withdraw 的路由
    pub fn new(amount: String, token_in: &SwapTokenInfo, token_out: &SwapTokenInfo) -> Self {
        let router = RouteInDex {
            dex_id: 0,
            pool_id: "".to_string(),
            in_token_symbol: token_in.symbol.clone(),
            in_token_addr: token_in.token_addr.clone(),
            out_token_symbol: token_out.symbol.clone(),
            out_token_addr: token_out.token_addr.clone(),
            zero_for_one: false,
            fee: "500".to_string(),
            amount_in: amount.clone(),
            min_amount_out: amount.clone(),
        };

        Self {
            amount_in: amount.clone(),
            amount_out: amount.clone(),
            percentage: "10000".to_string(),
            route_in_dex: vec![router],
        }
    }
}

// 子路由
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RouteInDex {
    // #[serde(rename = "dexId", alias = "dex_id")]
    pub dex_id: u16,
    // #[serde(rename = "poolId", alias = "pool_id")]
    pub pool_id: String,
    // #[serde(rename = "inTokenSymbol", alias = "in_token_symbol")]
    pub in_token_symbol: String,
    // #[serde(rename = "inTokenAddr", alias = "in_token_addr")]
    pub in_token_addr: String,
    // #[serde(rename = "outTokenSymbol", alias = "out_token_symbol")]
    pub out_token_symbol: String,
    // #[serde(rename = "outTokenAddr", alias = "out_token_addr")]
    pub out_token_addr: String,
    // #[serde(rename = "zeroForOne", alias = "zero_for_one")]
    pub zero_for_one: bool,
    pub fee: String,
    // #[serde(rename = "amountIn", alias = "amount_in")]
    pub amount_in: String,
    // #[serde(rename = "minAmountOut", alias = "min_amount_out")]
    pub min_amount_out: String,
}

#[cfg(test)]
mod tests {
    use super::{ApproveReq, QuoteReq};
    use wallet_database::entities::bill::NewBillEntity;

    fn approve_req(value: &str) -> ApproveReq {
        ApproveReq {
            contract: "contract".to_string(),
            from: "from".to_string(),
            spender: "spender".to_string(),
            value: value.to_string(),
            chain_code: "ethereum".to_string(),
        }
    }

    #[test]
    fn approve_bill_rejects_invalid_amount_without_panicking() {
        let result = NewBillEntity::try_from(approve_req("invalid"));

        assert!(result.is_err());
    }

    #[test]
    fn approve_bill_rejects_non_finite_amount() {
        let result = NewBillEntity::try_from(approve_req("NaN"));

        assert!(result.is_err());
    }

    #[test]
    fn addr_tron_to_eth_converts_tron_base58_address() {
        let converted = QuoteReq::addr_tron_to_eth("TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR").unwrap();

        assert_eq!(
            converted.to_string().to_lowercase(),
            "0x891cdb91d149f23b1a45d9c5ca78a88d0cb44c18"
        );
    }
}
