use super::BackendApi;
use crate::{
    request::{SignedTranAcceptReq, SignedTranCreateReq, SignedTranUpdateHashReq, SyncBillReq},
    response::BackendResponse,
    response_vo::{chain::GasOracle, transaction::RecordResp},
};
use std::collections::HashMap;

impl BackendApi {
    pub async fn signed_tran_create(
        &self,
        req: &SignedTranCreateReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("signed/trans/create")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_tran_accept(
        &self,
        req: &SignedTranAcceptReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("signed/trans/accept")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_tran_update_trans_hash(
        &self,
        req: &SignedTranUpdateHashReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("signed/trans/updateTransdHash")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn gas_oracle(&self, chain_code: &str) -> Result<GasOracle, crate::Error> {
        let mut params = HashMap::new();
        params.insert("chainCode", chain_code);

        let res = self
            .client
            .post("token/findGasTracker")
            .json(params)
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn record_lists(
        &self,
        chain_code: &str,
        address: &str,
        start_time: Option<String>,
    ) -> Result<RecordResp, crate::Error> {
        let req = SyncBillReq::new(chain_code, address, start_time);

        let res = self
            .client
            .post("account/record/list")
            .json(req)
            .send::<BackendResponse>()
            .await?;

        res.process()
    }
}

#[cfg(test)]
mod tests {
    use crate::request::SignedTranUpdateHashReq;

    use super::*;
    use crate::consts::BASE_URL;
    use serde_json::json;
    use wallet_utils::init_test_log;

    #[tokio::test]
    pub async fn test_signed_tran_create() {
        init_test_log();
        let tx_str = "xxxxxxxxxxxx";
        let req = SignedTranCreateReq {
            withdraw_id: "155061461155188736".to_string(),
            address: "TL5YGitvEyqUakseGRED2jDUJ8sv6qpLaR".to_string(),
            chain_code: "tron".to_string(),
            tx_str: tx_str.to_string(),
            raw_data: "".to_string(),
        };
        let api = BackendApi::new(None, None).unwrap();
        let res = api.signed_tran_create(&req).await.unwrap();
        tracing::info!("res  {:?}", res);
    }

    #[tokio::test]
    pub async fn test_fee_oracle() {
        init_test_log();

        let api = BackendApi::new(None, None).unwrap();
        let res = api.gas_oracle("eth").await.unwrap();
        tracing::info!("res  {:?}", res);
    }

    #[tokio::test]
    pub async fn test_signed_tran_accetp() {
        init_test_log();

        let address = vec![
            "TBA5hXR9mm6kpzFMwnh3dkqham4d9GQH8w".to_string(),
            "TJk5nUGoaMFmcrmSubFD11w6DVf5uX5yi6".to_string(),
            "TGyw6wH5UT5GVY5v6MTWedabScAwF4gffQ".to_string(),
        ];
        let str = r#"[{\"queue_id\":\"155061461155188736\",\"address\":\"TBA5hXR9mm6kpzFMwnh3dkqham4d9GQH8w\",\"signature\":\"be03fac326d23804cb97074743cd7b9a96bf5b3bebeb586bc100cc8d3e77dc29129ee748fa909131a84178c87e46abf84b5f5b8ba9c4492399c1bc7c406b9cd201\",\"status\":1},{\"queue_id\":\"155061461155188736\",\"address\":\"TJk5nUGoaMFmcrmSubFD11w6DVf5uX5yi6\",\"signature\":\"a26013db79e4ee9f24a84aa27d8ad3ccc68405b57baf8b8ed90e0829c3d3320e6adc3ea82a0867f05fdb0f2ab745aa3f40a767f806f328af8418ee137e376b5a01\",\"status\":1},{\"queue_id\":\"155061461155188736\",\"address\":\"TGyw6wH5UT5GVY5v6MTWedabScAwF4gffQ\",\"signature\":\"f9563beea8a050e8dac7917eb2e0ea002e8f819ae03a9e8fcefe887e5bbe872f2a5e6cc49783d635c9468fd0ef929355af012f8de6d02483fbffb35cedde03c901\",\"status\":1}]"#;
        let req = SignedTranAcceptReq {
            withdraw_id: "155061461155188736".to_string(),
            accept_address: address,
            tx_str: json!(str),
            status: 1,
            raw_data: "".to_string(),
        };

        let api = BackendApi::new(None, None).unwrap();
        let res = api.signed_tran_accept(&req).await.unwrap().unwrap();
        tracing::info!("res  {:?}", res);
    }

    #[tokio::test]
    pub async fn test_signed_tran_up_hash() {
        init_test_log();

        let req = SignedTranUpdateHashReq {
            withdraw_id: "155061461155188736".to_string(),
            hash: "xxxxxxxxxxxx".to_string(),
            remark: "hello".to_string(),
            raw_data: "".to_string(),
        };
        let api = BackendApi::new(None, None).unwrap();
        let res = api.signed_tran_update_trans_hash(&req).await.unwrap();
        tracing::info!("res  {:?}", res);
    }

    #[tokio::test]
    pub async fn test_records_list() {
        init_test_log();
        let base_url = BASE_URL.to_string();
        let backend = BackendApi::new(Some(base_url), None).unwrap();
        let cc = backend
            .record_lists(
                "tron",
                "TDmNZ4Wz7aMEt1tbRq7EVocWkxWn2SLoPE",
                Some("2024-09-28 00:00:00".to_string()),
            )
            .await
            .unwrap();

        println!("[record] res: {cc:#?}");
    }
}
