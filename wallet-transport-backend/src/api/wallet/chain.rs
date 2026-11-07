use crate::{
    consts::endpoint::CHAIN_LIST,
    request::{ChainListReq, ChainRpcListReq},
    response_vo::chain::{ChainInfos, ChainList},
};

use crate::api::BackendApi;

impl BackendApi {
    pub async fn chain_default_list(&self) -> Result<serde_json::Value, crate::Error> {
        self.post_backend_empty("chain/defaultList").await
    }

    pub async fn chain_list(&self, req: ChainListReq) -> Result<ChainList, crate::Error> {
        self.post_backend_json(CHAIN_LIST, serde_json::json!(req)).await
    }

    pub async fn _chain_list(&self) -> Result<serde_json::Value, crate::Error> {
        self.post_backend_empty("chain/list").await
    }

    pub async fn chain_rpc_list(&self, req: ChainRpcListReq) -> Result<ChainInfos, crate::Error> {
        self.post_backend_json("chain/rpcList", serde_json::json!(req)).await
    }
}
