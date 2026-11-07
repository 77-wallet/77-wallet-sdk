use crate::{
    api::BackendApi,
    response::response::BackendResponse,
    response_vo::stake::{DelegateOrderArgs, DelegateQueryResp},
};

impl BackendApi {
    pub async fn delegate_order(
        &self,

        account: &str,
        energy: i64,
    ) -> Result<DelegateQueryResp, crate::Error> {
        let req = DelegateOrderArgs { address: account.to_string(), energy_amount: energy };

        let res = self.client.post("delegate/order").json(req).send::<BackendResponse>().await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn delegate_query_order(
        &self,

        order_id: &str,
    ) -> Result<DelegateQueryResp, crate::Error> {
        let endpoint = format!("/delegate/order/{}", order_id);
        let res = self.client.post(&endpoint).send::<BackendResponse>().await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn delegate_is_open(&self) -> Result<bool, crate::Error> {
        let res = self.client.post("delegate/isOpen").send::<BackendResponse>().await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn delegate_complete(&self, order_id: &str) -> Result<(), crate::Error> {
        let endpoint = format!("/delegate/complete/{}", order_id);
        let _res = self.client.post(&endpoint).send::<BackendResponse>().await?;
        let _ = _res.process::<Option<bool>>(&self.aes_cbc_cryptor)?;
        Ok(())
    }

    pub async fn vote_list(&self) -> Result<crate::response_vo::stake::VoteListResp, crate::Error> {
        let res = self.client.post("vote/list").send::<BackendResponse>().await?;
        res.process(&self.aes_cbc_cryptor)
    }
}
