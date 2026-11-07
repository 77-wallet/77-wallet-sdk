use std::{env, path::PathBuf};

use tokio::io::{AsyncBufReadExt, BufReader};
use wallet_api::{
    api_wallet_console::{
        commands::ConsoleCommand,
        events::{
            AccountAddressRow, ApiWalletRow, BalanceAssetRow, ClientRuntimeInfo, WithdrawOrderRow,
        },
        wallet_import::import_configured_api_wallets,
        worker_protocol::WorkerEvent,
    },
    batch_transfer::{collect_target_addresses, run_batch_transfer},
    dirs::Dirs,
    messaging::notify::FrontendNotifyEvent,
    testkit::env::get_manager_with_config,
};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = run().await {
        emit(&WorkerEvent::Failed { error: err.to_string() });
    }
}

async fn run() -> anyhow::Result<()> {
    let mut config_file = "config.toml".to_string();
    let mut password = "q1111111".to_string();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                if let Some(value) = args.next() {
                    config_file = value;
                }
            }
            "--password" => {
                if let Some(value) = args.next() {
                    password = value;
                }
            }
            _ => {}
        }
    }

    let (wallet_manager, test_params) = get_manager_with_config(&config_file).await?;
    init_worker_log(&config_file, &test_params.device_req.sn).await;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(&password).await;

    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    wallet_manager.set_frontend_notify_sender(notify_tx).await?;
    emit(&WorkerEvent::Started);
    emit(&WorkerEvent::RuntimeInfoLoaded {
        info: ClientRuntimeInfo {
            device_sn: test_params.device_req.sn.clone(),
            device_type: test_params.device_req.device_type.clone(),
            device_app_id: test_params.device_req.app_id.clone(),
            package_id: test_params.device_req.package_id.clone(),
            app_version: test_params.device_req.app_version.clone(),
        },
    });

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        tokio::select! {
            notify = notify_rx.recv() => {
                match notify {
                    Some(event) => {
                        let payload = serde_json::to_string(&event)
                            .unwrap_or_else(|err| format!("notify serialize failed: {err}"));
                        emit(&WorkerEvent::Notify { payload });
                    }
                    None => break,
                }
            }
            line = lines.next_line() => {
                let Some(line) = line? else {
                    break;
                };
                match serde_json::from_str::<ConsoleCommand>(&line) {
                    Ok(command) => handle_command(
                        &wallet_manager,
                        &test_params,
                        &config_file,
                        command,
                    )
                    .await,
                    Err(err) => emit(&WorkerEvent::Failed {
                        error: format!("invalid worker command: {err}"),
                    }),
                }
            }
        }
    }

    Ok(())
}

async fn init_worker_log(config_file: &str, sn: &str) {
    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let client_id = format!("test_data_{}", config_file.replace(".toml", ""));
    let storage_dir = PathBuf::from(manifest_dir).join("test_data").join(client_id);
    let Ok(dirs) = Dirs::new(&storage_dir.to_string_lossy()) else {
        return;
    };
    let _ = wallet_api::xlog::init_log(Some("info"), &"app_code", &dirs, sn).await;
}

async fn handle_command(
    wallet_manager: &wallet_api::manager::WalletManager,
    test_params: &wallet_api::testkit::env::TestParams,
    config_file: &str,
    command: ConsoleCommand,
) {
    match command {
        ConsoleCommand::ImportConfiguredWallets => {
            match import_configured_api_wallets(wallet_manager, test_params, config_file).await {
                Ok(messages) => emit(&WorkerEvent::ImportFinished { messages }),
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::RefreshWallets => match wallet_manager.get_api_wallet_list().await {
            Ok(list) => {
                let mut wallets = Vec::new();
                for item in list.0 {
                    if let Some(wallet) = item.recharge_wallet {
                        wallets.push(ApiWalletRow {
                            role: "subwallet".to_string(),
                            name: wallet.name,
                            address: wallet.address,
                            uid: wallet.uid,
                        });
                    }
                    if let Some(wallet) = item.withdraw_wallet {
                        wallets.push(ApiWalletRow {
                            role: "withdraw".to_string(),
                            name: wallet.name,
                            address: wallet.address,
                            uid: wallet.uid,
                        });
                    }
                }
                emit(&WorkerEvent::WalletsLoaded { wallets });
            }
            Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
        },
        ConsoleCommand::RefreshAccountAddresses { wallet_address, chain_code } => {
            let res = wallet_manager
                .list_api_wallet_account(&wallet_address, None, Some(chain_code.clone()), 0, 500)
                .await;
            match res {
                Ok(accounts) => {
                    let mut rows = Vec::new();
                    for account in accounts.data {
                        for chain in
                            account.chain.into_iter().filter(|chain| chain.chain_code == chain_code)
                        {
                            rows.push(AccountAddressRow {
                                wallet_address: wallet_address.clone(),
                                account_id: account.account_id,
                                name: account.name.clone(),
                                chain_code: chain.chain_code,
                                address: chain.address,
                            });
                        }
                    }
                    emit(&WorkerEvent::AccountAddressesLoaded { wallet_address, rows });
                }
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::RefreshBalances { wallet_address, account_id, chain_code } => {
            emit_balance_assets(wallet_manager, wallet_address, account_id, chain_code).await;
        }
        ConsoleCommand::SyncWalletAssets { wallet_address } => {
            emit(&WorkerEvent::Log {
                message: format!("starting on-chain asset sync for {wallet_address}"),
            });
            match wallet_manager
                .sync_api_assets_by_wallet(wallet_address.clone(), None, vec![])
                .await
            {
                Ok(()) => emit(&WorkerEvent::Log {
                    message: format!("on-chain asset sync completed for {wallet_address}"),
                }),
                Err(err) => emit(&WorkerEvent::Log {
                    message: format!("on-chain asset sync failed for {wallet_address}: {err}"),
                }),
            }
            emit_balance_assets(wallet_manager, wallet_address, None, None).await;
        }
        ConsoleCommand::ImportBind { app_id, org_id, subaccount_uid, withdrawal_uid } => {
            let res = wallet_manager
                .import_bind(
                    &test_params.device_req.sn,
                    &org_id,
                    &app_id,
                    &subaccount_uid,
                    &withdrawal_uid,
                )
                .await;
            match res {
                Ok(resp) => {
                    emit(&WorkerEvent::Log { message: format!("import_bind ok: {resp:?}") });
                    emit(&WorkerEvent::BindFinished);
                }
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::ScanBind { app_id, org_id, subaccount_uid, withdrawal_uid } => {
            let res =
                wallet_manager.scan_bind(&app_id, &org_id, &subaccount_uid, &withdrawal_uid).await;
            match res {
                Ok(resp) => {
                    emit(&WorkerEvent::Log { message: format!("scan_bind ok: {resp:?}") });
                    emit(&WorkerEvent::BindFinished);
                }
                Err(err) => {
                    let normalized: (i64, String) = err.into();
                    emit(&WorkerEvent::Log {
                        message: format!("scan_bind failed: {normalized:?}"),
                    });
                    emit(&WorkerEvent::BindFinished);
                }
            }
        }
        ConsoleCommand::FetchPendingWithdrawOrders { withdrawal_uid, page_size } => {
            let res = wallet_manager
                .page_api_withdraw_order(&withdrawal_uid, vec![0], 0, page_size)
                .await;
            match res {
                Ok(page) => {
                    let rows = page
                        .data
                        .into_iter()
                        .map(|order| WithdrawOrderRow {
                            trade_no: order.trade_no,
                            out_order_id: order.out_order_id,
                            client_id: order.client_id,
                            chain_code: order.chain_code,
                            symbol: order.symbol,
                            value: order.value,
                            from_addr: order.from_addr,
                            to_addr: order.to_addr,
                            status: format!("{:?}", order.status),
                        })
                        .collect::<Vec<_>>();
                    emit(&WorkerEvent::WithdrawOrdersLoaded { rows });
                }
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::ReviewWithdrawOrders { trade_nos, approve } => {
            let action = if approve { "approve" } else { "reject" };
            emit(&WorkerEvent::Log {
                message: format!("{action} withdraw orders start: {} orders", trade_nos.len()),
            });
            for trade_no in trade_nos {
                let res = if approve {
                    wallet_manager.sign_api_withdrawal_order(&trade_no).await
                } else {
                    wallet_manager.reject_api_withdrawal_order(&trade_no).await
                };
                match res {
                    Ok(resp) => emit(&WorkerEvent::Log {
                        message: format!("{action} withdraw order {trade_no} ok: {resp:?}"),
                    }),
                    Err(err) => emit(&WorkerEvent::Log {
                        message: format!("{action} withdraw order {trade_no} failed: {err}"),
                    }),
                }
            }
            emit(&WorkerEvent::Log { message: format!("{action} withdraw orders finished") });
            emit(&WorkerEvent::WithdrawReviewFinished);
        }
        ConsoleCommand::SetChainNode { chain_code, rpc_url } => {
            let result = set_chain_node_by_rpc_url(wallet_manager, &chain_code, &rpc_url).await;
            match result {
                Ok(()) => emit(&WorkerEvent::ChainNodeSet { chain_code, rpc_url }),
                Err(err) => emit(&WorkerEvent::ChainNodeSetFailed { error: err.to_string() }),
            }
        }
        ConsoleCommand::RefreshTaskQueueStatus => {
            match wallet_manager.get_console_task_queue_summary().await {
                Ok(summary) => emit(&WorkerEvent::TaskQueueStatusLoaded { summary }),
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::DiagnoseTrade { trade_no } => {
            match wallet_manager.diagnose_trade(&trade_no).await {
                Ok(diagnostics) => emit(&WorkerEvent::TradeDiagnosticsLoaded { diagnostics }),
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::LoadTransferTargets { chain_code, sub_wallet_address } => {
            let res = wallet_manager
                .list_api_wallet_account(
                    &sub_wallet_address,
                    None,
                    Some(chain_code.clone()),
                    0,
                    500,
                )
                .await
                .map(|subaccounts| collect_target_addresses(subaccounts.data, &chain_code));
            match res {
                Ok(targets) => emit(&WorkerEvent::LoadedTargets { targets }),
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
        ConsoleCommand::RunBatchTransfer { config } => {
            match run_batch_transfer(wallet_manager.clone(), &config).await {
                Ok(summary) => {
                    for item in summary.logs.iter() {
                        emit(&WorkerEvent::Log {
                            message: format!(
                                "{} {} - {}",
                                item.to_address,
                                if item.success { "OK" } else { "FAILED" },
                                item.detail
                            ),
                        });
                    }
                    emit(&WorkerEvent::TransferFinished { summary });
                }
                Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
            }
        }
    }
}

async fn emit_balance_assets(
    wallet_manager: &wallet_api::manager::WalletManager,
    wallet_address: String,
    account_id: Option<u32>,
    chain_code: Option<String>,
) {
    let res = wallet_manager
        .get_api_assets_list(&wallet_address, account_id, chain_code, None, false)
        .await;
    match res {
        Ok(assets) => {
            let rows = assets
                .0
                .into_iter()
                .map(|asset| BalanceAssetRow {
                    wallet_address: wallet_address.clone(),
                    account_id,
                    chain_code: asset.chain_code,
                    symbol: asset.symbol,
                    name: asset.name,
                    amount: asset.balance.amount,
                    currency: asset.balance.currency,
                    fiat_value: asset.balance.fiat_value,
                })
                .collect::<Vec<_>>();
            emit(&WorkerEvent::BalanceAssetsLoaded { wallet_address, rows });
        }
        Err(err) => emit(&WorkerEvent::Failed { error: err.to_string() }),
    }
}

async fn set_chain_node_by_rpc_url(
    wallet_manager: &wallet_api::manager::WalletManager,
    chain_code: &str,
    rpc_url: &str,
) -> anyhow::Result<()> {
    let target = normalize_rpc_url(rpc_url);
    let node = wallet_manager
        .get_node_list(chain_code)
        .await?
        .into_iter()
        .find(|node| normalize_rpc_url(&node.rpc_url) == target)
        .ok_or_else(|| anyhow::anyhow!("configured node not found: {chain_code} {rpc_url}"))?;

    wallet_manager.set_chain_node(chain_code, &node.node_id).await?;
    Ok(())
}

fn normalize_rpc_url(rpc_url: &str) -> &str {
    rpc_url.trim().trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::normalize_rpc_url;

    #[test]
    fn rpc_url_matching_ignores_whitespace_and_trailing_slash() {
        assert_eq!(normalize_rpc_url(" https://nile.trongrid.io/ "), "https://nile.trongrid.io");
    }
}

fn emit(event: &WorkerEvent) {
    match serde_json::to_string(event) {
        Ok(line) => println!("{line}"),
        Err(err) => println!(
            "{}",
            serde_json::json!({
                "Failed": { "error": format!("worker event serialize failed: {err}") }
            })
        ),
    }
}
