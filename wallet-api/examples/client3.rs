#![allow(dead_code)]
#![allow(unused)]

use tokio_stream::StreamExt as _;
#[cfg(any(feature = "integration-tests", test))]
use wallet_api::testkit::env::{TestParams, get_manager};
use wallet_api::{
    manager::WalletManager,
    messaging::notify::FrontendNotifyEvent,
    request::{
        api_wallet::{
            account::CreateApiAccountReq, trans::ApiBaseTransferReq, transfer::ApiTransferExReq,
        },
        transaction::BaseTransferReq,
    },
    xlog::init_log,
};
#[cfg(not(any(feature = "integration-tests", test)))]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::warn!("client3 example requires feature `integration-tests`");
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let _ = _tx;
    let _ = _rx;
    Ok(())
}
use wallet_database::entities::{
    api_wallet::ApiWalletType, api_withdraw::ApiWithdrawStatus, asset_token_key::AssetTokenKey,
};
use wallet_transport_backend::request::api_wallet::strategy::{ChainConfig, IndexAndAddress};
use wallet_types::chain::chain::ChainCode;

async fn run_collect_strategy(
    wallet_manager: &WalletManager,
) -> Result<(), Box<dyn std::error::Error>> {
    let wallet_uid = "28eadfc4105d274e97add4350aaf4069f797a3e0b12a37fdd8555c988ff64856";
    let res = wallet_manager
        .update_collect_strategy(
            &wallet_uid,
            1,
            vec![ChainConfig {
                chain_code: ChainCode::Ethereum.to_string(),
                chain_address_type: None,
                normal_address: IndexAndAddress {
                    index: Some(0),
                    address: "0xd743cb69b376fb8b3f25c53e7b2d806fd4ef74f7".to_string(),
                },
                risk_address: IndexAndAddress {
                    index: Some(1),
                    address: "0xd743cb69b376fb8b3f25c53e7b2d806fd4ef74f7".to_string(),
                },
            }],
        )
        .await;
    match res {
        Ok(reason) => {
            tracing::info!("更新归集策略成功 --------------------- ");
        }
        Err(err) => {
            tracing::error!("更新归集策略失败 --------------------- 5: {err:#?}");
        }
    }

    let res = wallet_manager.get_collect_strategy(wallet_uid).await?;
    tracing::info!("get collect strategy -------------------- {:?}", res);
    Ok(())
}

async fn run_withdrawal_strategy(
    wallet_manager: &WalletManager,
) -> Result<(), Box<dyn std::error::Error>> {
    let wallet_uid = "5f58a308c1ee00c7d0d39a4e9f7482d4069e58116a52ed674bd85d073a9f9bb2";
    let res = wallet_manager
        .update_withdrawal_strategy(
            &wallet_uid,
            5,
            vec![ChainConfig {
                chain_code: ChainCode::Ethereum.to_string(),
                chain_address_type: None,
                normal_address: IndexAndAddress {
                    index: Some(0),
                    address: "0x4DffcD64054D82ab2D433daD4BFB742182dd9E95".to_string(),
                },
                risk_address: IndexAndAddress {
                    index: Some(1),
                    address: "0x4AEc3e3FD46E6349F6004c67f8Ed9C8a277f9946".to_string(),
                },
            }],
        )
        .await;
    match res {
        Ok(reason) => {
            tracing::info!("更新提币策略成功 --------------------- ");
        }
        Err(err) => {
            tracing::error!("更新提币策略失败 --------------------- 5: {err:#?}");
        }
    }

    let res = wallet_manager.get_withdrawal_strategy(wallet_uid).await?;
    tracing::info!("get withdrawal strategy -------------------- {:?}", res);
    Ok(())
}

async fn run_withdraw_order(
    wallet_manager: &WalletManager,
) -> Result<(), Box<dyn std::error::Error>> {
    let trade_no = "W1979072898652102656";

    wallet_manager.sign_api_withdrawal_order(trade_no).await?;
    tracing::info!("同意提币成功 --------------------- ");

    // wallet_manager.reject_api_withdrawal_order(trade_no).await?;
    // tracing::info!("拒绝提币策略成功 --------------------- ");

    let uid = "78da2f53e3ee6c651859b557a1c74067d6c44db356f0b2835c09c03f8541f78a";
    let res = wallet_manager
        .page_api_withdraw_order(
            uid,
            vec![ApiWithdrawStatus::AuditReject as u8, ApiWithdrawStatus::SendingTxFailed as u8],
            0,
            10,
        )
        .await?;
    for e in &res.data {
        let res = serde_json::to_string(e).unwrap();
        tracing::info!("-------- {:?}", res);
    }
    tracing::info!("获取提币拒绝策略数据 --------------------- {:?}", res);
    Ok(())
}

async fn run_tx(wallet_manager: &WalletManager) -> Result<(), Box<dyn std::error::Error>> {
    // 获取订单记录
    // let order_list = wallet_manager.list_api_withdraw_order(&wallet_uid).await?;
    // tracing::info!("order_list ------------------- 2: {order_list:#?}");

    // 绑定钱包
    // let key = "app_id";
    // let merchain_id = "test_merchain";

    //
    // let res = wallet_manager.bind_merchant(key, merchain_id, uid).await;
    // tracing::info!("res --------------------- 3: {res:?}");

    // bnb
    // let from = "0x4f31D44C05d6fDce4db64da2E9601BeE8ad9EA5e";
    // let to = "0xF97c59fa5C130007CF51286185089d05FE45B69e";

    // tron
    // let from = "TLAedgzGJWA9seJYbBTTMWNtxoKooapq6n";
    // let to = "TNRUkgGzhwuRL2rGeFPErThYWr4MranYLA";

    // sol
    // let from = "DF3Nong1byLe4Nb1Qu4R8T4G7TFDpLe7T58moGbUotpe";
    // let to = "J8ByH2pUySpXL4fXdgPpwnaL7R381xunqXT2cqaZ1tm";

    // ton
    // let from = "UQBTmOIHin7OrxheQ979Y3_xJjHxMUJocknrv3_J_dCocuqy";
    // let to = "0QDex-zBG6cbJCwaxA7999xIB_ZhNAwOr37lsw5HxB7Ldrpq";

    // sui
    // let from = "0xb69713b670ba3bfcfa7ea577005de40bf026e277b574773bc4c6f0adb7e1ced8";
    // let to = "0xd830497ecd7321d4e0e501d3f71689380e8e8883ee5e1597cf06b3b72a95d226";

    // let value = "0.000001";
    // let trade_no = "0x0000000125";
    // let res1 = wallet_manager
    //     .api_withdrawal_order(from, to, value, "bnb", None, "BNB", trade_no, 1, &wallet_uid)
    //     .await;
    // tracing::info!("api_withdrawal_order ------------------- 4: {res1:#?}");
    Ok(())
}

#[cfg(any(feature = "integration-tests", test))]
async fn create_wallet(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
    wallet_password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 创建钱包
    // let language_code = 1;
    // let phrase = &test_params.create_wallet_req.phrase;
    // let salt = "q1111111";
    // let wallet_name = "api_wallet";
    // let binding_address = None;
    // let wallet_uid = wallet_manager
    //     .create_api_wallet(
    //         language_code,
    //         phrase,
    //         salt,
    //         wallet_name,
    //         wallet_password,
    //         None,
    //         ApiWalletType::SubAccount,
    //         binding_address,
    //     )
    //     .await?;
    // tracing::info!("子wallet创建成功 ------------------------ 1: {wallet_uid:#?}");

    // let res = wallet_manager.get_api_wallet_list().await?;
    // tracing::info!("get withdraw wallet list ------------------------ 2: {res:#?}");

    // let salt1 = "q1111112";
    // let binding_address = None;
    // let withdrawal_uid = wallet_manager
    //     .create_api_wallet(
    //         language_code,
    //         phrase,
    //         salt1,
    //         wallet_name,
    //         wallet_password,
    //         None,
    //         ApiWalletType::Withdrawal,
    //         binding_address,
    //     )
    //     .await?;
    // tracing::info!("withdraw wallet 创建成功 ------------------------ 2: {withdrawal_uid:#?}");

    // let wallet_uid = "d7212497905e693951ebdeafe8c5846323f8f5a620a0b28347616e49c1445144";
    // let withdrawal_uid = "886f6c36bdc992962cc65ad1debf76f6f21da0c2fb3c67509596405b45e7d1da";
    // let res = wallet_manager
    //     .scan_bind(
    //         "ad14fc378b244647b23efe9ec3271992",
    //         "69142a7e704e424777cecc3f",
    //         &wallet_uid,
    //         &withdrawal_uid,
    //     )
    //     .await?;
    // tracing::info!("绑定app成功 ------------------- 3: {res:#?}");
    Ok(())
}

#[cfg(any(feature = "integration-tests", test))]
async fn import_wallet(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
    wallet_password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let language_code = 1;
    let phrase = &test_params.create_wallet_req.phrase;
    let salt = "q1111111";
    let wallet_name = "api_wallet";
    // let uid = wallet_manager.import_api_wallet(language_code, phrase, salt, wallet_name, wallet_password, None, ApiWalletType::SubAccount, None).await?;
    // let li = wallet_manager.get_api_wallet_list().await?;
    // let lo = li.0;
    // let i = &lo[0];
    // let recharge_wallet = &i.recharge_wallet.as_ref().unwrap().address;
    let recharge_wallet = "0xaa9A1FDB5155be28C68e935CA85ACD70b858FAc1";
    let salt1 = "q1111112";
    wallet_manager
        .import_api_wallet(
            language_code,
            phrase,
            salt1,
            wallet_name,
            wallet_password,
            None,
            ApiWalletType::Withdrawal,
            Some(recharge_wallet),
        )
        .await?;
    Ok(())
}

#[cfg(any(feature = "integration-tests", test))]
async fn run(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
) -> Result<(), Box<dyn std::error::Error>> {
    let wallet_password = "";
    wallet_manager.set_passwd_cache(wallet_password).await?;
    tracing::info!("绑定钱包之前必须设置密码成功 ------------------------ ");

    import_wallet(wallet_manager, test_params, wallet_password).await?;

    // wallet_manager
    //     .edit_api_account_name(2, "0xa3dAEDC43D1a131b27B22B01D93E15B63583955A", "你还是娃娃")
    //     .await?;

    // run_collect_strategy(wallet_manager).await?;
    // run_withdrawal_strategy(wallet_manager).await?;
    // run_withdraw_order(wallet_manager).await?;

    Ok(())
}

#[cfg(any(feature = "integration-tests", test))]
async fn run_get_withdraw_list(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
) -> Result<(), Box<dyn std::error::Error>> {
    // 获取待审核列表
    // let uid = "87d59aacda3df0da102d3ccc340a45e793ebd7ac1e07f96099f4311864278164";
    // let res = wallet_manager
    //     .page_api_withdraw_order_with_init_status(
    //         uid,
    //         ApiWithdrawStatus::Init as u8, // 待审核
    //         vec![6],
    //         0,
    //         10,
    //     )
    //     .await?;
    // tracing::info!("----------------- {:?}", res);
    // for e in &res.data {
    //     let res = serde_json::to_string(e).unwrap();
    //     tracing::info!("-------- {:?}", res);
    // }

    let res = wallet_manager
        .api_bill_lists(
            Some("0x8dBcdD3923408AB55BB9fCA34d5fC1aD24099480".to_string()),
            None,                    // account
            None,                    // mulit
            None,                    // addr
            Some("eth".to_string()), // chain
            None,
            Some(true),
            None,
            None,
            vec![4],
            None,
            0,
            20,
        )
        .await?;
    tracing::info!("----------------- {:?}", res);
    for e in &res.data {
        let res = serde_json::to_string(e).unwrap();
        tracing::info!("-------- {:?}", res);
    }

    // let res = wallet_manager
    //     .api_bill_detail(
    //         "c56479fc2f5da82a540c1252dcae0663dbd0e8ee52c9d1387653d8954789c258",
    //         "TSdB5jJpdBGZLKHA1CpQeb3S5ZcVF9dceG",
    //     )
    //     .await;
    // tracing::info!("get withdraw list --------------------------- 2: {res:#?}");

    tracing::info!("api_recent_bill");
    let res = wallet_manager
        .api_recent_bill("", "0x4DffcD64054D82ab2D433daD4BFB742182dd9E95", "eth", 0, 20)
        .await?;
    tracing::info!("----------------- {:?}", res);

    // let res = wallet_manager.api_query_tx_result(vec!["3".to_string()]).await?;
    // tracing::info!("api_query_tx_result: {res:#?}");

    Ok(())
}

#[cfg(any(feature = "integration-tests", test))]
async fn run_transfer(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
) -> Result<(), Box<dyn std::error::Error>> {
    // 提币
    let wallet_password = "";
    let res = wallet_manager
        .api_transfer(ApiTransferExReq {
            base: ApiBaseTransferReq {
                from: "0x4DffcD64054D82ab2D433daD4BFB742182dd9E95".to_string(),
                to: "0xd743cb69b376fb8b3f25c53e7b2d806fd4ef74f7".to_string(),
                value: "0.00194".to_string(),
                chain_code: "eth".to_string(),
                symbol: "ETH".to_string(),
                request_resource_id: None,
                decimals: 18,
                token_address: AssetTokenKey::Native,
                spend_all: false,
                notes: None,
                metadata: None,
            },
            password: wallet_password.to_string(),
            fee_setting: "".to_string(),
            signer: None,
        })
        .await?;
    tracing::info!("tx_hash: {}", res.tx_hash);
    Ok(())
}

#[cfg(any(feature = "integration-tests", test))]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (wallet_manager, test_params): (wallet_api::manager::WalletManager, TestParams) =
        get_manager().await?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    wallet_manager.set_frontend_notify_sender(tx).await?;

    let res = wallet_manager.set_invite_code(Some("I1912683353004912640".to_string())).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
    tracing::info!("set_invite_code ------------------------0: {res:?}");

    let res = wallet_manager.init_api_swap().await;
    match res {
        Ok(_) => {}
        Err(err) => {
            tracing::error!(" =========================== run {}", err);
            return Err(err.into());
        }
    }

    let res = run(&wallet_manager, &test_params).await;
    match res {
        Ok(_) => {}
        Err(err) => {
            tracing::error!(" =========================== run {}", err)
        }
    }

    // tracing::info!("------------------------------------- list");
    // let res = run_get_withdraw_list(&wallet_manager, &test_params).await;
    // match res {
    //     Ok(_) => {}
    //     Err(err) => {
    //         tracing::error!(" =========================== run_get_withdraw_list {}", err);
    //         return Err(err);
    //     }
    // }

    // tracing::info!("------------------------------------- list");
    // let res = run_transfer(&wallet_manager, &test_params).await;
    // match res {
    //     Ok(_) => {}
    //     Err(err) => {
    //         tracing::error!(" =========================== run_transfer {}", err)
    //     }
    // }

    let chains = wallet_manager.sync_api_chains().await?;
    if !chains.is_empty() {
        wallet_manager.sync_api_wallet_chain_data().await?;
    }

    loop {
        tokio::select! {
            msg = rx.next() => {
                tracing::info!("data: {msg:?}");
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("ctrl_c");
                let _ = wallet_manager.close().await;
                break;
            }
        }
    }
    Ok(())
}
