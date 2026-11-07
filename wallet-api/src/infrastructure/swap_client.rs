// 先放在这里，不知道最终会不会和后端用一个
use crate::{
    error::service::ServiceError, request::transaction::DexRoute,
    response_vo::standard_wallet::swap::SwapTokenInfo,
};
use alloy::primitives::U256;
use wallet_chain_interact::sol::protocol::Instruction;
use wallet_transport::client::HttpClient;

pub struct SwapClient {
    client: HttpClient,
}

impl SwapClient {
    pub fn new(url: &str) -> Result<Self, ServiceError> {
        let timeout = Some(std::time::Duration::from_secs(20));
        let client = HttpClient::new(url, None, timeout)?;

        Ok(Self { client })
    }

    // - **10000**: dex_id 不支持
    // - **10100**: token 不支持
    // - **10200**: token pair之间没有兑换路径
    // - **10300**: 池子不支持, 获取到的池子不在我们的支持列表中
    // - **10400**: 池子获取失败
    // - **10500**: 流动性拆分失败, 也可以显示为流动性不足
    // - **20000**: 其它不常见的特殊错误, 按照错误消息字符串显示
    // - **30000**: 未知错误, 保留, 按照错误消息字符串显示
    fn match_error(&self, res: AggregatorResp) -> ServiceError {
        let code = match res.code {
            10100 => 4402,
            10200 | 10300 | 10400 | 10500 => 4403,
            _ => -1,
        };
        crate::error::service::ServiceError::AggregatorError {
            code,
            agg_code: res.code,
            msg: res.msg.unwrap_or_default(),
        }
    }

    fn handle_result<T: serde::de::DeserializeOwned>(
        &self,
        res: AggregatorResp,
    ) -> Result<T, ServiceError> {
        if res.code == 200 {
            Ok(wallet_utils::serde_func::serde_from_value::<T>(res.data)?)
        } else {
            Err(self.match_error(res))
        }
    }

    pub async fn get_quote(&self, req: AggQuoteRequest) -> Result<AggQuoteResp, ServiceError> {
        let res = self.client.post_request::<_, AggregatorResp>("quote", req).await?;

        self.handle_result::<AggQuoteResp>(res)
    }

    pub async fn sol_instructions(
        &self,
        req: SolInstructionReq,
    ) -> Result<SolInstructResp, crate::error::service::ServiceError> {
        let res =
            self.client.post_request::<_, AggregatorResp>("get_sol_dexswap_instrs", req).await?;

        self.handle_result::<SolInstructResp>(res)
    }
}

// 响应
#[derive(Debug, serde::Deserialize)]
pub struct AggregatorResp {
    pub data: serde_json::Value,
    pub code: i32,
    pub msg: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AggQuoteRequest {
    pub chain_code: String,
    // 处理精度后的值
    pub amount: String,
    pub unique: String,
    pub in_token_addr: String,
    pub out_token_addr: String,
    pub dex_ids: Vec<DexId>,
}

#[derive(Debug, serde::Serialize)]
pub struct DexId {
    pub dex_id: i32,
}

// 查询报价的响应
#[derive(Debug, serde::Deserialize)]
pub struct AggQuoteResp {
    pub chain_code: String,
    pub amount_in: String,
    pub amount_out: String,
    pub dex_route_list: Vec<DexRoute>,
    pub default_slippage: u64,
}
impl AggQuoteResp {
    pub fn amount_out_u256(&self) -> Result<U256, crate::error::service::ServiceError> {
        Ok(wallet_utils::unit::u256_from_str(&self.amount_out)?)
    }

    pub fn get_slippage(&self) -> f64 {
        self.default_slippage as f64 / 10000.0
    }
}

// 默认的兑换比例
#[derive(serde::Serialize)]
pub struct DefaultQuoteResp {
    pub token_in: SwapTokenInfo,
    pub token_out: SwapTokenInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SolInstructionReq {
    pub unique: String,
    pub payer: String,
    pub is_native_token: bool,
    pub amount_in: String,
    pub amount_out: String,
    pub dex_route_list: Vec<DexRoute>,
    pub is_simulate: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SolInstructResp {
    pub ins: Vec<Instruction>,
    pub alts: Vec<String>,
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::{AggQuoteRequest, DexId, SwapClient};

    fn client() -> SwapClient {
        SwapClient::new("http://127.0.0.1:28888/api").unwrap()
    }

    #[tokio::test]
    async fn test_quote() {
        let client = client();

        let dex_id1 = DexId { dex_id: 2 };
        let dex_id2 = DexId { dex_id: 3 };

        let req = AggQuoteRequest {
            chain_code: "eth".to_string(),
            amount: "10000000000000000".to_string(),
            in_token_addr: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(),
            out_token_addr: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
            dex_ids: vec![dex_id1, dex_id2],
            unique: "1".to_string(),
        };
        let res = client.get_quote(req).await.unwrap();

        println!("res: {res:?}");
    }
}
