use serde_json::json;

use super::BackendApi;
use crate::{
    response::BackendResponse,
    response_vo::{
        address::AddressUidList,
        multisig::{DepositAddress, MultisigServiceFees},
    },
    FindAddressRawDataRes, SignedCreateOrderReq, SignedOrderAcceptReq, SignedSaveAddressReq,
    SignedUpdateRechargeHashReq, SignedUpdateSignedHashReq, SingedOrderCancelReq,
};

impl BackendApi {
    pub async fn address_find_address_raw_data(
        &self,
        req: crate::request::FindAddressRawDataReq,
    ) -> Result<FindAddressRawDataRes, crate::Error> {
        let res = self
            .client
            .post("signed/order/findAddressRawData")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn signed_find_address(
        &self,
        req: crate::request::SignedFindAddressReq,
    ) -> Result<DepositAddress, crate::Error> {
        let res = self
            .client
            .post("signed/order/findAddress")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;

        res.process()
    }

    pub async fn signed_fee_list(
        &self,
        req: crate::request::SignedFeeListReq,
    ) -> Result<MultisigServiceFees, crate::Error> {
        let res = self
            .client
            .post("/signed/order/feeList")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_order_update_signed_hash(
        &self,
        req: &SignedUpdateSignedHashReq,
    ) -> Result<Option<String>, crate::Error> {
        let res = self
            .client
            .post("/signed/order/updateSignedHash")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_order_update_recharge_hash(
        &self,
        req: &SignedUpdateRechargeHashReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("/signed/order/updateRechargeHash")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_order_save_confirm_address(
        &self,
        req: SignedSaveAddressReq,
    ) -> Result<Option<String>, crate::Error> {
        let res = self
            .client
            .post("/signed/order/saveConfirmAddress")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn get_address_uid(
        &self,
        chain_code: String,
        address: Vec<String>,
    ) -> Result<AddressUidList, crate::Error> {
        let req = json!({
            "addressList": address,
            "chainCode": chain_code
        });

        let res = self
            .client
            .post("/keys/queryUidByAddress")
            .json(req)
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_order_cancel(
        &self,
        req: &SingedOrderCancelReq,
    ) -> Result<Option<String>, crate::Error> {
        let res = self
            .client
            .post("/signed/order/cancel")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_order_create(
        &self,
        req: SignedCreateOrderReq,
    ) -> Result<String, crate::Error> {
        let res = self
            .client
            .post("/signed/order/create")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn signed_order_accept(
        &self,
        req: &SignedOrderAcceptReq,
    ) -> Result<Option<()>, crate::Error> {
        let res = self
            .client
            .post("/signed/order/accept")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    // /signed/order/success
    pub async fn signed_order_success(
        &self,
        req: SignedUpdateRechargeHashReq,
    ) -> Result<String, crate::Error> {
        let res = self
            .client
            .post("/signed/order/accept")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    // cancel multisig queue
    pub async fn signed_trans_cancel(
        &self,
        queue_id: &str,
        raw_data: String,
    ) -> Result<(), crate::Error> {
        let req = serde_json::json!({
            "withdrawId":queue_id.to_string(),
            "rawData":raw_data
        });

        self.post_request::<_, ()>("/signed/trans/cancel", req)
            .await
    }
}

#[cfg(test)]
mod test {
    use wallet_utils::init_test_log;

    use crate::{
        api::BackendApi,
        request::{FindAddressRawDataReq, SignedFeeListReq, SignedFindAddressReq},
        SignedCreateOrderReq, SignedUpdateRechargeHashReq,
    };

    #[tokio::test]
    async fn test_address_find_address_raw_data() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let uid = "598e4144d26d871676e266036af660b3b38d38ea670a0abbfb75effab60890ad";
        let typ = None;
        let raw_time = None;

        let req = FindAddressRawDataReq::new(uid, typ, raw_time);
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .address_find_address_raw_data(req)
            .await
            .unwrap();

        println!("[test_address_find_address_raw_data] res: {res:#?}");
    }

    #[tokio::test]
    async fn test_signed_order_create() {
        init_test_log();
        let base_url = "http://api.wallet.net";

        let chain_code = "bnb_test";
        let address = "0x5985CE40d3dACf7c1352e464691BC7fb03215928";
        let multisig_address = "0x1C2Ce4352f86D37715EA3a8De1D7122ff8760149";

        let req = SignedCreateOrderReq::new(chain_code, address, multisig_address)
            .with_elements(&1.to_string());
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .signed_order_create(req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_signed_find_address() {
        let base_url = "http://api.wallet.net";

        let req = SignedFindAddressReq {
            name: None,
            code: None,
            chain_code: "tron".to_string(),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .signed_find_address(req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_signed_fee_list() {
        let base_url = "http://api.wallet.net";

        let req = SignedFeeListReq {
            chain_code: "eth".to_string(),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .signed_fee_list(req)
            .await
            .unwrap();

        println!("[test_chain_default_list] res: {res:?}");
    }

    #[tokio::test]
    async fn signed_order_update_recharge_hash() {
        init_test_log();
        let base_url = "http://api.wallet.net";
        let req = SignedUpdateRechargeHashReq {
            order_id: "66a1b2da6a5fb47fea0e00fa".to_string(),
            hash: "0ba4f88de631c5218503d37d520e815f40b5d3499b86a7029c15c70e9a379873".to_string(),
            product_code: "1".to_string(),
            receive_chain_code: "tron".to_string(),
            receive_address: "".to_string(),
            raw_data: "".to_string(),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .signed_order_update_recharge_hash(&req)
            .await
            .unwrap();
        println!("[test_signed_order_update_signed_hash] res: {res:?}");
    }
}
