use crate::{
    consts::endpoint::api_wallet::{
        TRANS_EVENT_ACK, TRANS_EXECUTE_COMPLETE, TRANS_SERVICE_FEE_TRANS,
    },
    request::api_wallet::transaction::*,
};
use wallet_ecdh::GLOBAL_KEY;

use crate::{api::BackendApi, api_request::ApiBackendRequest};

impl BackendApi {
    // 归集打手续费记录上传
    pub async fn upload_service_fee_record(
        &self,
        req: &ServiceFeeUploadReq,
    ) -> Result<Option<()>, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(TRANS_SERVICE_FEE_TRANS, api_req).await
    }

    // 交易执行回执上传
    pub async fn upload_tx_exec_receipt(
        &self,
        req: &TxExecReceiptUploadReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(TRANS_EXECUTE_COMPLETE, api_req).await?;
        Ok(())
    }

    // 交易记录恢复
    pub async fn restore_transaction_records(
        &self,
        _req: &RestoreTxRecordsReq,
    ) -> Result<Option<()>, crate::Error> {
        todo!()
    }

    // 收到交易事件确认
    pub async fn trans_event_ack(
        &self,
        req: &TransEventAckReq,
    ) -> Result<Option<()>, crate::Error> {
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(TRANS_EVENT_ACK, api_req).await
    }
}
