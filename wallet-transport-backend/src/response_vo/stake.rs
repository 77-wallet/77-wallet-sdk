#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateOrderArgs {
    pub address: String,
    pub energy_amount: i64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateQueryResp {
    #[serde(default)]
    pub order_id: String,
    pub energy_hash: Option<String>,
    pub energy_status: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemEnergyResp {
    // 用户单日能量补贴总次数
    pub time_limit: i64,
    // 用户单日剩余总次数
    pub left_times: i64,
    // 当笔交易最大补贴能量
    pub energy_limit: i64,
    // 当前可用能量)
    pub left_energy: i64,
    // 领取状态
    pub status: bool,
    // 剩余奖励的次数
    pub left_award_energy_times: i64,
    // 平台剩余奖励次数
    pub left_platform_energy_times: i64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteListResp {
    pub node_resp_list: Vec<NodeRespList>,
    pub total_vote_count: i64,
    pub total_node: u16,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRespList {
    pub name: Option<String>,
    pub address: String,
    #[serde(default)]
    pub vote_count: i64,
    pub url: String,
    /// 分成比例
    pub brokerage: f64,
    /// 年化收益率
    pub apr: f64,
}
