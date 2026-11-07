use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiChainListResp(pub Vec<ApiChainItem>);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiChainItem {
    create_time: Option<String>,
    update_time: Option<String>,
    id: Option<String>,
    /// 链名称
    pub name: String,
    #[serde(rename = "code")]
    pub chain_code: String,
    /// 链id
    chain_id: Option<i64>,
    /// 线上块高
    #[serde(default)]
    global_height: Option<i64>,
    /// 本地块高
    #[serde(default)]
    local_height: Option<i64>,
    /// 启用
    #[serde(default)]
    pub enable: bool,
    /// 排序
    seq: Option<i64>,
    /// 默认链
    #[serde(default)]
    default_chain: Option<bool>,
    /// TG监控通知
    #[serde(default)]
    tg_alert: Option<bool>,
    /// 查看链上地址URL
    #[serde(default)]
    address_url: String,
    /// 查看链上hash URL
    hash_url: Option<String>,
    /// 查看链上合约地址URL
    #[serde(default)]
    token_url: Option<String>,
    /// 启用块高监听开关
    #[serde(default)]
    enable_block: Option<bool>,
    /// 出块时间 例：2秒，1小时，针对tg
    block_time: Option<String>,
    /// 主币编码
    pub master_token_code: Option<String>,
    /// 版本
    pub app_version_code: String,
}
