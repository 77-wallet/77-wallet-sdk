use crate::api::BackendApi;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApproveSaveParams {
    pub uid: String,
    pub index: i32,
    #[serde(rename = "chainCode")]
    pub chain_code: String,
    pub spender: String,
    #[serde(rename = "ownerAddress")]
    pub owner_address: String,
    #[serde(rename = "tokenAddr")]
    pub token_addr: String,
    pub status: String,
    pub value: String,
    #[serde(rename = "limitType")]
    pub limit_type: String,
    pub hash: String,
}

impl ApproveSaveParams {
    pub fn new(
        index: i32,
        uid: &str,
        chain_code: &str,
        spender: &str,
        owner_address: &str,
        token_addr: &str,
        value: String,
        hash: &str,
        limit_type: &str,
    ) -> Self {
        Self {
            uid: uid.to_owned(),
            index,
            chain_code: chain_code.to_owned(),
            spender: spender.to_owned(),
            owner_address: owner_address.to_owned(),
            token_addr: token_addr.to_owned(),
            status: "APPROVED".to_string(),
            value,
            limit_type: limit_type.to_owned(),
            hash: hash.to_owned(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackApproveList {
    pub list: Vec<ApproveInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApproveInfo {
    pub id: String,
    pub uid: String,
    pub index: u32,
    #[serde(rename = "chainCode")]
    pub chain_code: String,
    pub spender: String,
    #[serde(rename = "ownerAddress")]
    pub owner_address: String,
    #[serde(rename = "tokenAddr")]
    pub token_addr: String,
    pub hash: String,
    pub status: String,
    pub value: String,
    #[serde(rename = "limitType")]
    pub limit_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveCancelReq {
    pub spender: String,
    pub token_addr: String,
    pub owner_address: String,
    pub tx_hash: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportChain {
    pub support_chain: Vec<ChainDex>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainDex {
    pub chain_code: String,
    #[serde(alias = "swapContractAddr")]
    pub aggregator_addr: String,
    pub dexs: Vec<DexInfo>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DexInfo {
    pub dex_id: u64,
    pub dex_name: String,
    pub icon_code: String,
}

impl BackendApi {
    pub async fn approve_list(
        &self,
        uid: String,
        index: i32,
    ) -> Result<BackApproveList, crate::Error> {
        let endpoint = "swap/approve/list";

        let req = std::collections::HashMap::from([("uid", uid), ("index", index.to_string())]);

        self.post_request::<_, BackApproveList>(endpoint, &req).await
    }

    pub async fn update_used_approve(&self, ids: Vec<String>) -> Result<bool, crate::Error> {
        let endpoint = "/swap/approve/usedAllQuota";
        let req = std::collections::HashMap::from([("ids", ids)]);
        self.post_request::<_, bool>(endpoint, &req).await
    }

    pub async fn support_chain_list(&self) -> Result<SupportChain, crate::Error> {
        let endpoint = "chain/swapSupportChain/list";
        let req = json!({});
        self.post_request::<_, SupportChain>(endpoint, &req).await
    }

    pub async fn support_chain_list_v2(
        &self,
        app_version: String,
    ) -> Result<SupportChain, crate::Error> {
        let endpoint = "chain/swapSupportChain/v2/list";
        let req = json!({
            "appVersionCode": app_version
        });
        self.post_request::<_, SupportChain>(endpoint, &req).await
    }
}
