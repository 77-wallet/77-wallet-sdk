use crate::get_manager;
use anyhow::Result;
use wallet_api::request::{
    stake::{CancelAllUnFreezeReq, FreezeBalanceReq, UnFreezeBalanceReq, WithdrawBalanceReq},
    transaction::Signer,
};
use wallet_database::entities::bill::BillKind;

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_account_resource() -> Result<()> {
    let manager = get_manager().await;
    let account = "TGtSVaqXzzGM2XgbUvgZzZeNqFwp1VvyXS".to_string();
    let res = manager.resource_info(account).await?;

    tracing::info!("resource = {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_freeze_fee() -> Result<()> {
    let manager = get_manager().await;
    let req = FreezeBalanceReq {
        owner_address: "TMrVocuPpNqf3fpPSSWy7V8kyAers3p1Jc".to_string(),
        resource: "energy".to_string(),
        frozen_balance: 50,
        signer: None,
    };

    let bill_kind = BillKind::FreezeBandwidth.to_i8() as i64;

    let content = serde_json::to_string(&req).unwrap();
    let res = manager.estimate_stake_fee(bill_kind, content).await?;

    tracing::info!("fee {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_freeze() -> Result<()> {
    let manager = get_manager().await;

    let _signer =
        Signer { address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(), permission_id: 3 };
    let _signer = None;

    let req = FreezeBalanceReq {
        owner_address: "TMrVocuPpNqf3fpPSSWy7V8kyAers3p1Jc".to_string(),
        resource: "energy".to_string(),
        frozen_balance: 50,
        signer: _signer,
    };
    let password = "123456".to_string();

    let res = manager.freeze_balance(req, password).await?;
    tracing::info!("freeze = {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_unfreeze_fee() -> Result<()> {
    let manager = get_manager().await;

    let req = UnFreezeBalanceReq {
        owner_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        resource: "bandwidth".to_string(),
        unfreeze_balance: 50,
        signer: None,
    };

    let bill_kind = BillKind::UnFreezeBandwidth.to_i8() as i64;
    let content = serde_json::to_string(&req).unwrap();

    let res = manager.estimate_stake_fee(bill_kind, content).await?;

    tracing::info!("unfreeze  {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_unfreeze() -> Result<()> {
    let manager = get_manager().await;

    let req = UnFreezeBalanceReq {
        owner_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        resource: "energy".to_string(),
        unfreeze_balance: 10,
        signer: None,
    };

    let password = "123456".to_string();
    let res = manager.un_freeze_balance(req, password).await?;

    tracing::info!("unfreeze  {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_freeze_list() -> Result<()> {
    let manager = get_manager().await;

    let owner = "TGtSVaqXzzGM2XgbUvgZzZeNqFwp1VvyXS".to_string();
    let res = manager.freeze_list(owner).await?;
    tracing::info!("freeze  {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_un_freeze_list() -> Result<()> {
    let manager = get_manager().await;

    let owner = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
    let res = manager.un_freeze_list(owner).await?;
    tracing::info!("unfreeze = {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_cancel_all_fee() -> Result<()> {
    let manager = get_manager().await;

    let req = CancelAllUnFreezeReq {
        owner_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        signer: None,
    };

    let bill_kind = BillKind::CancelAllUnFreeze.to_i8() as i64;
    let content = serde_json::to_string(&req).unwrap();

    let res = manager.estimate_stake_fee(bill_kind, content).await?;
    tracing::info!("unfreeze {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_cancel_all_unfreeze() -> Result<()> {
    let manager = get_manager().await;

    let req = CancelAllUnFreezeReq {
        owner_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        signer: None,
    };
    let password = "123456".to_string();

    let res = manager.cancel_all_unfreeze(req, password).await?;
    tracing::info!("unfreeze {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_withdraw() -> Result<()> {
    let manager = get_manager().await;

    let req = WithdrawBalanceReq {
        owner_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        signer: None,
    };

    let password = "123456".to_string();
    let res = manager.withdraw_unfreeze(req, password).await?;
    tracing::info!("withdraw {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// #[tokio::test]
// async fn test_trx_to_resource() {
//     let manager = get_manager().await;

//     let account = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
//     let value = 100;
//     let resource_type = "energy".to_string();

//     let res = manager.trx_to_resource(account, value, resource_type).await;
//     tracing::info!("response = {}", serde_json::to_string(&res).unwrap());
// }

// #[tokio::test]
// async fn test_resource_to_trx() {
//     let manager = get_manager().await;

//     let account = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
//     let value = 7200;
//     let resource_type = "bandwidth".to_string();

//     let res = manager.resource_to_trx(account, value, resource_type).await;
//     tracing::info!("response = {}", serde_json::to_string(&res).unwrap());
// }

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_system_resource() -> Result<()> {
    let manager = get_manager().await;

    let address = "TTD5EM94SmLPSTyvzjiisjB71QCD4vHQcm".to_string();
    let res = manager.system_resource(address).await?;
    tracing::info!("system resource = {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_request_energy() -> Result<()> {
    let manager = get_manager().await;

    let address = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
    let energy = 50;

    let res = manager.request_energy(address, energy).await?;
    tracing::info!("request {}", serde_json::to_string(&res).unwrap());
    Ok(())
}
