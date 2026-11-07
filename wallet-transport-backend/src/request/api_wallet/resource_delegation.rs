/// 资源委托申请相关的数据结构
/// 用于 SDK 主动向后端申请资源委托的接口
use serde::{Deserialize, Serialize};

use super::transaction::TransType;

/// 资源类型枚举
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ResourceType {
    /// 能量资源
    #[serde(rename = "ENERGY")]
    Energy,
    /// 带宽资源
    #[serde(rename = "BANDWIDTH")]
    Bandwidth,
}

impl ResourceType {
    /// 获取资源类型的字符串表示
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceType::Energy => "ENERGY",
            ResourceType::Bandwidth => "BANDWIDTH",
        }
    }
}

/// 资源委托申请请求结构体
/// 对应后端接口: POST /aw/trans/resourceDl/apply
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceApplyReq {
    /// 平台交易单号（归集单或提币单）
    #[serde(rename = "tradeNo")]
    pub trade_no: String,
    /// 应用 appId
    #[serde(rename = "appId")]
    pub app_id: String,
    /// 商户 ID
    #[serde(rename = "orgId")]
    pub org_id: String,
    /// 链编码（可选）
    pub chain: Option<String>,
    /// 申请的资源换算成本币的数量，TRON 资源代理按整 TRX 执行
    #[serde(rename = "nativeTokenAmount")]
    pub native_token_amount: i64,
    /// 代理的资源数量（可选）
    #[serde(rename = "resourceAmount")]
    pub resource_amount: Option<f64>,
    /// 资源类型（ENERGY / BANDWIDTH）
    #[serde(rename = "resourceType")]
    pub resource_type: ResourceType,
    /// 接收资源的地址
    pub to: String,
    /// 交易类型（COL_RSC_DL / WD_RSC_DL）
    pub r#type: TransType,
}

impl ResourceApplyReq {
    /// 创建资源委托申请请求
    ///
    /// # 参数
    ///
    /// * `trade_no` - 平台交易单号
    /// * `app_id` - 应用 appId
    /// * `org_id` - 商户 ID
    /// * `chain` - 链编码（可选）
    /// * `native_token_amount` - 申请的资源换算成本币的数量，整 TRX
    /// * `resource_amount` - 代理的资源数量（可选）
    /// * `resource_type` - 资源类型
    /// * `to` - 接收资源的地址
    /// * `r#type` - 交易类型
    pub fn new(
        trade_no: &str,
        app_id: &str,
        org_id: &str,
        chain: Option<&str>,
        native_token_amount: i64,
        resource_amount: Option<f64>,
        resource_type: ResourceType,
        to: &str,
        r#type: TransType,
    ) -> Self {
        Self {
            trade_no: trade_no.to_string(),
            app_id: app_id.to_string(),
            org_id: org_id.to_string(),
            chain: chain.map(|s| s.to_string()),
            native_token_amount,
            resource_amount,
            resource_type,
            to: to.to_string(),
            r#type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ResourceApplyReq, ResourceType};
    use crate::request::api_wallet::transaction::TransType;

    #[test]
    fn resource_apply_req_serializes_integer_native_amount() {
        let req = ResourceApplyReq::new(
            "C1",
            "app",
            "org",
            Some("tron"),
            197,
            Some(14650.0),
            ResourceType::Energy,
            "receiver",
            TransType::Col,
        );

        let value = serde_json::to_value(req).expect("serialize request");
        assert_eq!(value["nativeTokenAmount"], 197);
        assert_eq!(value["resourceAmount"], 14650.0);
    }
}

/// 资源委托申请响应结构体
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResourceDlRep {
    /// 申请结果（成功/失败）
    #[serde(rename = "dlRes")]
    pub dl_res: Option<bool>,
    /// 资源单号（申请成功时返回）
    #[serde(rename = "dlTradeNo")]
    pub dl_trade_no: Option<String>,
}

impl ApplyResourceDlRep {
    /// 判断申请是否成功
    pub fn is_success(&self) -> bool {
        self.dl_res.unwrap_or(false)
    }
}
