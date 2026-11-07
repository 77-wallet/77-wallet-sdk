use crate::get_manager;
use anyhow::Result;
use wallet_api::{
    domain,
    request::permission::{KeysReq, PermissionReq},
};

// 权限列表
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_permission_list() -> Result<()> {
    let wallet_manager = get_manager().await;

    let res = wallet_manager.permission_list()?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 账户权限
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_permission_accounts() -> Result<()> {
    let wallet_manager = get_manager().await;

    let address = "TFtvHtfuLo5xJJe9HpSAEaEi4bzT8Eeyu2".to_string();
    let res = wallet_manager.account_permission(address).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// manager permission
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_manager_permission() -> Result<()> {
    let wallet_manager = get_manager().await;
    let address = "TKDDywzwyYJD8n1BMy5cqr7cxjEFaKJ8h3".to_string();
    let res = wallet_manager.manager_permission(address).await?;
    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 新增权限手续费
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_add_permission_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let keys = vec![
        KeysReq { address: "TJkMavCTA2qd7TLzWehtMnKnxkSeaWAdcq".to_string(), weight: 1 },
        KeysReq { address: "TE4xhjv6dvEbYxXGjP4ntnN3viSN9Nf8Qv".to_string(), weight: 116 },
        KeysReq { address: "TQsWaoYYwZ4EVj9wgDG4bfdjwYYRejQsTC".to_string(), weight: 14 },
    ];

    let req = PermissionReq {
        grantor_addr: "TQsWaoYYwZ4EVj9wgDG4bfdjwYYRejQsTC".to_string(),
        name: "转账、质押,12".to_string(),
        active_id: Some(8),
        threshold: 1,
        operations: vec![1, 2, 3, 5],
        keys,
    };

    let res = wallet_manager.modify_permission_fee(req, "update".to_string()).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 新增权限
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_add_permission() -> Result<()> {
    let wallet_manager = get_manager().await;

    let keys =
        vec![KeysReq { address: "TLK9t3ht5GE1oYPx8pdoG1PScQdJgS7Pwb".to_string(), weight: 1 }];

    let req = PermissionReq {
        grantor_addr: "TQnSwWGaFkT2zjumDJkbaFi4uRAvEq4An1".to_string(),
        name: "????".to_string(),
        active_id: None,
        threshold: 1,
        operations: vec![1, 0, 2, 5, 12, 16, 48, 58],
        keys,
    };
    let password = "123456".to_string();

    let res = wallet_manager.modify_permission(req, "new".to_string(), password).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 修改权限
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_up_permission() -> Result<()> {
    let wallet_manager = get_manager().await;

    let keys = vec![
        KeysReq { address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(), weight: 1 },
        KeysReq { address: "TKDDywzwyYJD8n1BMy5cqr7cxjEFaKJ8h3".to_string(), weight: 1 },
        KeysReq { address: "TNcRALWJNRtM5zfLQFvbiuycue9ZcxFFjQ".to_string(), weight: 1 },
    ];

    let req = PermissionReq {
        grantor_addr: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(),
        name: "update name".to_string(),
        active_id: Some(2),
        threshold: 2,
        operations: vec![1, 54, 55, 59, 56, 57, 58],
        keys,
    };
    let password = "123456".to_string();

    let res = wallet_manager.modify_permission(req, "update".to_string(), password).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 删除权限
#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_del_permission() -> Result<()> {
    let wallet_manager = get_manager().await;

    let keys =
        vec![KeysReq { address: "TNPTj8Dbba6YxW5Za6tFh6SJMZGbUyucXQ".to_string(), weight: 1 }];

    let req = PermissionReq {
        grantor_addr: "TUe3T6ErJvnoHMQwVrqK246MWeuCEBbyuR".to_string(),
        name: "修改权限".to_string(),
        active_id: Some(4),
        threshold: 1,
        operations: vec![1],
        keys,
    };
    let password = "123456".to_string();

    let res = wallet_manager.modify_permission(req, "delete".to_string(), password).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_build_multisig_queue() -> Result<()> {
    let wallet_manager = get_manager().await;

    let keys = vec![
        KeysReq { address: "TWtoyV1B5z33PNU5BGzAMgcu2NQzctbgSv".to_string(), weight: 1 },
        KeysReq { address: "TNAAhuax96f8j1Azy2kVayYVcBCW8y6aYo".to_string(), weight: 1 },
    ];

    let req = PermissionReq {
        grantor_addr: "TQnSwWGaFkT2zjumDJkbaFi4uRAvEq4An1".to_string(),
        name: "picker2.0".to_string(),
        active_id: Some(2),
        threshold: 2,
        operations: vec![1, 54, 55, 59, 56, 57, 58],
        keys,
    };
    let password = "123456".to_string();
    let expiration = 5;

    let res =
        wallet_manager.build_multisig_queue(req, "new".to_string(), password, expiration).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_recover_data() {
    let wallet_manager = get_manager().await;

    let uids = vec!["TL55jNbXWeM6se5fpKBQTTmH45HZ7stvW3".to_string()];
    domain::permission::PermissionDomain::recover_permission(wallet_manager.ctx(), uids)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires configured backend, chain state, and seeded addresses"]
async fn test_backup() {
    let _wallet_manager = get_manager().await;

    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
    tracing::info!("xxx")
}
