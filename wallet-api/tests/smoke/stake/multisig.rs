use crate::get_manager;
use anyhow::Result;
use wallet_api::request::{
    stake::{DelegateReq, FreezeBalanceReq, UnDelegateReq, VoteWitnessReq, VotesReq},
    transaction::Signer,
};
use wallet_database::entities::bill::BillKind;

#[tokio::test]
#[ignore = "requires backend, multisig state, and seeded stake wallet"]
async fn test_build_freeze() -> Result<()> {
    let manager = get_manager().await;

    let _singer =
        Signer { address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(), permission_id: 3 };
    let _signer = None;

    let req = FreezeBalanceReq {
        owner_address: "TQnSwWGaFkT2zjumDJkbaFi4uRAvEq4An1".to_string(),
        resource: "bandwidth".to_string(),
        frozen_balance: 300,
        signer: _signer,
    };

    let bill_kind = BillKind::FreezeEnergy.to_i8() as i64;
    let content = serde_json::to_string(&req).unwrap();

    let password = "123456".to_string();
    let res = manager.build_multisig_stake(bill_kind, content, 1, password).await?;

    tracing::info!("delegate {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, multisig state, and seeded stake wallet"]
async fn test_build_delegate() -> Result<()> {
    let manager = get_manager().await;

    let req = DelegateReq {
        owner_address: "TNPTj8Dbba6YxW5Za6tFh6SJMZGbUyucXQ".to_string(),
        receiver_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        balance: 100000,
        resource: "energy".to_string(),
        lock: false,
        lock_period: 10000000.0,
        signer: None,
    };

    let bill_kind = BillKind::DelegateEnergy.to_i8() as i64;
    let content = serde_json::to_string(&req).unwrap();

    let password = "123456".to_string();
    let res = manager.build_multisig_stake(bill_kind, content, 1, password).await?;

    tracing::info!("delegate {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, multisig state, and seeded stake wallet"]
async fn test_build_un_delegate() -> Result<()> {
    let manager = get_manager().await;

    let req = UnDelegateReq {
        owner_address: "TNPTj8Dbba6YxW5Za6tFh6SJMZGbUyucXQ".to_string(),
        receiver_address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        balance: 3,
        resource: "energy".to_string(),
        signer: None,
    };

    let bill_kind = BillKind::UnDelegateEnergy.to_i8() as i64;
    let content = serde_json::to_string(&req).unwrap();

    let password = "123456".to_string();
    let res = manager.build_multisig_stake(bill_kind, content, 1, password).await?;

    tracing::info!("delegate {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, multisig state, and seeded stake wallet"]
async fn test_build_vote() -> Result<()> {
    let manager = get_manager().await;

    let owner_address = "TFdDqaoMkPbWWv9EUTbmfGP142f9ysiJq2";
    let req = VoteWitnessReq::new(
        owner_address,
        vec![VotesReq::new("TA4pHhHgobzSGH3CWPsZ5URNk3QkzUEggX", 1, "name")],
        None,
    );
    let bill_kind = BillKind::Vote.to_i8() as i64;
    let content = serde_json::to_string(&req).unwrap();

    let password = "123456".to_string();
    let res = manager.build_multisig_stake(bill_kind, content, 1, password).await?;

    tracing::info!("vote {}", serde_json::to_string(&res).unwrap());
    Ok(())
}
