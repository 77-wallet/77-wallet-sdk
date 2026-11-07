use crate::init;
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::request::api_wallet::{
    audit::AuditResultReportReq,
    swap::ApiInitSwapReq,
    transaction::{TransAckType, TransEventAckReq, TransStatus, TransType, TxExecReceiptUploadReq},
};

#[serial_test::serial]
#[tokio::test]
async fn test_trans_event_ack() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let req = TransEventAckReq::new("C2036337093101867008", TransType::Col, TransAckType::TxFeeRes);
    let res = backend_api.trans_event_ack(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_fetch_all_api_tokens] res: {res}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_reject_api_withdrawal_order() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let req = AuditResultReportReq::new("W2020535510761119744".to_string(), false, "reject");
    let res = backend_api.report_audit_result(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_fetch_all_api_tokens] res: {res}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_upload_tx_exec_receipt() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let from = Some("0x37D9A67696956F67F1Bdd302A79460c1266b8F1F");
    let to = Some("0x5A99406CE8D9F8B3527a38408582872144C8b890");
    let trade_no = "C2036586295360655360";
    let typ = TransType::Col;
    let hash = Some("");
    let status = TransStatus::Fail;
    let remark = "";
    let req = TxExecReceiptUploadReq::new(from, to, trade_no, typ, hash, status, remark);
    let res = backend_api.upload_tx_exec_receipt(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_upload_tx_exec_receipt] res: {res}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_upload_tx_exec_receipt_fee() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let from = Some("0x5A99406CE8D9F8B3527a38408582872144C8b890");
    let to = Some("0x37D9A67696956F67F1Bdd302A79460c1266b8F1F");
    let trade_no = "CF2036586325321883648";
    let typ = TransType::ColFee;
    let hash = None;
    let status = TransStatus::Fail;
    let remark = "";
    let req = TxExecReceiptUploadReq::new(from, to, trade_no, typ, hash, status, remark);
    let res = backend_api.upload_tx_exec_receipt(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_upload_tx_exec_receipt] res: {res}");
    Ok(())
}
