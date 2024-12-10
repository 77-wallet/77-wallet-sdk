use super::BackendApi;
use crate::{
    response::BackendResponse,
    response_vo::stake::{DelegateOrderArgs, DelegateQueryResp},
};

impl BackendApi {
    pub async fn delegate_order(
        &self,
        account: &str,
        energy: i64,
    ) -> Result<DelegateQueryResp, crate::Error> {
        let req = DelegateOrderArgs {
            address: account.to_string(),
            energy_amount: energy,
        };

        let res = self
            .client
            .post("delegate/order")
            .json(req)
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn delegate_query_order(
        &self,
        order_id: &str,
    ) -> Result<DelegateQueryResp, crate::Error> {
        let endpoint = format!("/delegate/order/{}", order_id);
        let res = self
            .client
            .post(&endpoint)
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn delegate_is_open(&self) -> Result<bool, crate::Error> {
        let res = self
            .client
            .post("delegate/isOpen")
            .send::<BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn delegate_complete(&self, order_id: &str) -> Result<bool, crate::Error> {
        let endpoint = format!("/delegate/complete/{}", order_id);
        let res = self
            .client
            .post(&endpoint)
            .send::<BackendResponse>()
            .await?;
        res.process()
    }
}

#[cfg(test)]
mod test {
    use crate::api::BackendApi;
    use crate::consts::BASE_URL;
    use wallet_utils::init_test_log;

    #[tokio::test]
    async fn test_delegate_is_open() {
        init_test_log();
        let base_url = BASE_URL;
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .delegate_is_open()
            .await
            .unwrap();
        tracing::info!("{res:?}")
    }

    #[tokio::test]
    async fn test_delegate_complete() {
        init_test_log();
        let base_url = BASE_URL;
        let order = "672343049017657afff102f1";
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .delegate_complete(&order)
            .await
            .unwrap();
        tracing::info!("{res:?}")
    }

    #[tokio::test]
    async fn test_delegate_query_order() {
        init_test_log();
        let base_url = "http://api.wallet.net";
        let order = "66e6b46c3ebdf9433dcb3c49";
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .delegate_query_order(&order)
            .await
            .unwrap();
        tracing::info!("{res:?}")
    }

    #[tokio::test]
    async fn test_delegate_order() {
        init_test_log();
        let base_url = BASE_URL;
        let address = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1";
        let energy = 10000;
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .delegate_order(&address, energy)
            .await
            .unwrap();
        tracing::info!("{res:?}")
    }
}
