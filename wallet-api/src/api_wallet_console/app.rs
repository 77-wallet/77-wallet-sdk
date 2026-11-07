use std::{collections::HashSet, sync::mpsc};

use eframe::egui;

use crate::{
    batch_transfer::BatchTransferSummary,
    response_vo::standard_wallet::diagnostics::{ConsoleTaskQueueSummary, TradeDiagnostics},
};

use super::{
    bind::BindForm,
    client::{ClientId, ClientState, ClientStatus},
    commands::ConsoleCommand,
    events::{AccountAddressRow, ApiWalletRow, BalanceAssetRow, ConsoleEvent, WithdrawOrderRow},
    session::spawn_client_session,
    transfer::TransferForm,
    ui,
};

pub struct ApiWalletConsoleApp {
    pub(crate) clients: Vec<ClientState>,
    pub(crate) selected_client: ClientId,
    pub(crate) bind_form: BindForm,
    pub(crate) bind_forms: Vec<BindForm>,
    pub(crate) transfer_form: TransferForm,
    pub(crate) transfer_forms: Vec<TransferForm>,
    pub(crate) account_address_rows: Vec<AccountAddressRow>,
    pub(crate) api_wallet_rows: Vec<ApiWalletRow>,
    pub(crate) balance_asset_rows: Vec<BalanceAssetRow>,
    pub(crate) account_addresses_only: String,
    pub(crate) account_address_status: String,
    pub(crate) comma_copy_start_account_id: String,
    pub(crate) comma_copy_count: String,
    pub(crate) withdraw_review_trade_nos: String,
    pub(crate) diagnostic_trade_no: String,
    pub(crate) pending_withdraw_orders: Vec<WithdrawOrderRow>,
    pub(crate) selected_withdraw_trade_nos: HashSet<String>,
    pub(crate) task_queue_summary: Option<ConsoleTaskQueueSummary>,
    pub(crate) trade_diagnostics: Option<TradeDiagnostics>,
    pub(crate) logs: Vec<String>,
    pub(crate) log_panel_widths: Vec<f32>,
    pub(crate) log_filter: LogFilter,
    pub(crate) observe_tab: ObserveTab,
    pub(crate) last_summary: Option<BatchTransferSummary>,
    pub(crate) is_busy: bool,
    pub(crate) show_transfer_confirm: bool,
    pub(crate) new_client_name: String,
    pub(crate) new_client_config: String,
    event_tx: mpsc::Sender<ConsoleEvent>,
    event_rx: mpsc::Receiver<ConsoleEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveTab {
    Runtime,
    Wallets,
    Balances,
    Accounts,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFilter {
    All,
    Selected,
    Notify,
    Error,
}

impl ApiWalletConsoleApp {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let clients = discover_clients();
        let transfer_forms =
            clients.iter().map(|client| TransferForm::for_client(&client.name)).collect::<Vec<_>>();
        let bind_forms =
            clients.iter().map(|client| BindForm::for_client(&client.name)).collect::<Vec<_>>();
        let client_count = clients.len();
        Self {
            clients,
            selected_client: ClientId(0),
            bind_form: bind_forms.first().cloned().unwrap_or_else(BindForm::default),
            bind_forms,
            transfer_form: transfer_forms.first().cloned().unwrap_or_else(TransferForm::default),
            transfer_forms,
            account_address_rows: Vec::new(),
            api_wallet_rows: Vec::new(),
            balance_asset_rows: Vec::new(),
            account_addresses_only: String::new(),
            account_address_status: "No account address query yet".to_string(),
            comma_copy_start_account_id: "1".to_string(),
            comma_copy_count: "10".to_string(),
            withdraw_review_trade_nos: String::new(),
            diagnostic_trade_no: String::new(),
            pending_withdraw_orders: Vec::new(),
            selected_withdraw_trade_nos: HashSet::new(),
            task_queue_summary: None,
            trade_diagnostics: None,
            logs: Vec::new(),
            log_panel_widths: vec![340.0; client_count],
            log_filter: LogFilter::All,
            observe_tab: ObserveTab::Wallets,
            last_summary: None,
            is_busy: false,
            show_transfer_confirm: false,
            new_client_name: String::new(),
            new_client_config: String::new(),
            event_tx,
            event_rx,
        }
    }

    pub(crate) fn selected_client(&self) -> Option<&ClientState> {
        self.clients.iter().find(|client| client.id == self.selected_client)
    }

    pub(crate) fn selected_client_mut(&mut self) -> Option<&mut ClientState> {
        self.clients.iter_mut().find(|client| client.id == self.selected_client)
    }

    pub(crate) fn selected_client_ready(&self) -> bool {
        self.selected_client().map(|client| client.status == ClientStatus::Ready).unwrap_or(false)
    }

    pub(crate) fn select_client(&mut self, client_id: ClientId) {
        if self.selected_client == client_id {
            return;
        }

        if let Some(slot) = self.transfer_forms.get_mut(self.selected_client.0) {
            *slot = self.transfer_form.clone();
        }
        if let Some(slot) = self.bind_forms.get_mut(self.selected_client.0) {
            *slot = self.bind_form.clone();
        }

        self.selected_client = client_id;
        self.transfer_form =
            self.transfer_forms.get(client_id.0).cloned().unwrap_or_else(TransferForm::default);
        self.bind_form =
            self.bind_forms.get(client_id.0).cloned().unwrap_or_else(BindForm::default);
        self.api_wallet_rows.clear();
        self.account_address_rows.clear();
        self.balance_asset_rows.clear();
        self.account_addresses_only.clear();
        self.account_address_status = "No account address query yet".to_string();
        self.last_summary = None;
        self.pending_withdraw_orders.clear();
        self.selected_withdraw_trade_nos.clear();
        self.task_queue_summary = None;
        self.trade_diagnostics = None;
        if let Some(client) = self.selected_client() {
            if let Some(command_tx) = client.command_tx.as_ref() {
                let _ = command_tx.send(ConsoleCommand::RefreshWallets);
            }
        }
    }

    pub(crate) fn append_log(&mut self, line: impl Into<String>) {
        self.logs.push(line.into());
        if self.logs.len() > 300 {
            self.logs.drain(0..self.logs.len() - 300);
        }
    }

    pub(crate) fn start_selected_client(&mut self) {
        let Some(client) = self.selected_client().cloned() else {
            self.append_log("No selected client");
            return;
        };
        if client.status == ClientStatus::Ready || client.status == ClientStatus::Starting {
            self.append_log(format!("{} is already {}", client.name, client.status.label()));
            return;
        }
        if let Some(client) = self.selected_client_mut() {
            client.status = ClientStatus::Starting;
            client.last_error = None;
        }
        self.is_busy = true;
        self.append_log(format!("{}: starting {}", client.name, client.config_file));

        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
        if let Some(selected) = self.selected_client_mut() {
            selected.command_tx = Some(command_tx);
        }
        spawn_client_session(client, command_rx, self.event_tx.clone());
    }

    pub(crate) fn import_wallets_for_selected_client(&mut self) {
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: import configured wallets requested", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::ImportConfiguredWallets);
    }

    pub(crate) fn refresh_wallets_for_selected_client(&mut self) {
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: api wallet DB refresh requested", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::RefreshWallets);
    }

    pub(crate) fn refresh_balances_for_wallet(&mut self, wallet_address: String) {
        if wallet_address.trim().is_empty() {
            self.append_log("Balance refresh skipped: wallet address is empty");
            return;
        }
        if let Some(client) = self.selected_client() {
            self.append_log(format!(
                "{}: balance refresh requested for {}",
                client.name, wallet_address
            ));
        }
        self.send_command_to_selected(ConsoleCommand::RefreshBalances {
            wallet_address,
            account_id: None,
            chain_code: None,
        });
    }

    pub(crate) fn sync_assets_for_wallet(&mut self, wallet_address: String) {
        if wallet_address.trim().is_empty() {
            self.append_log("Asset sync skipped: wallet address is empty");
            return;
        }
        if let Some(client) = self.selected_client() {
            self.append_log(format!(
                "{}: on-chain asset sync requested for {}",
                client.name, wallet_address
            ));
        }
        self.send_command_to_selected(ConsoleCommand::SyncWalletAssets { wallet_address });
    }

    pub(crate) fn import_bind_for_selected_client(&mut self) {
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: import_bind requested", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::ImportBind {
            app_id: self.bind_form.app_id(),
            org_id: self.bind_form.org_id(),
            subaccount_uid: self.bind_form.subaccount_uid(),
            withdrawal_uid: self.bind_form.withdrawal_uid(),
        });
    }

    pub(crate) fn scan_bind_for_selected_client(&mut self) {
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: scan_bind requested", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::ScanBind {
            app_id: self.bind_form.app_id(),
            org_id: self.bind_form.org_id(),
            subaccount_uid: self.bind_form.subaccount_uid(),
            withdrawal_uid: self.bind_form.withdrawal_uid(),
        });
    }

    pub(crate) fn use_nile_trongrid_for_selected_client(&mut self) {
        if let Some(client) = self.selected_client() {
            self.append_log(format!(
                "{}: switch TRON node to Nile TronGrid requested",
                client.name
            ));
        }
        self.send_command_to_selected(ConsoleCommand::SetChainNode {
            chain_code: "tron".to_string(),
            rpc_url: "https://nile.trongrid.io".to_string(),
        });
    }

    pub(crate) fn refresh_task_queue_for_selected_client(&mut self) {
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: task queue status refresh requested", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::RefreshTaskQueueStatus);
    }

    pub(crate) fn diagnose_trade_for_selected_client(&mut self) {
        let trade_no = self.diagnostic_trade_no.trim().to_string();
        if trade_no.is_empty() {
            self.append_log("Trade diagnostics skipped: trade_no is empty");
            return;
        }
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: trade diagnostics requested for {trade_no}", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::DiagnoseTrade { trade_no });
    }

    pub(crate) fn fetch_pending_withdraw_orders_for_selected_client(&mut self) {
        let withdrawal_uid = self.bind_form.withdrawal_uid();
        if withdrawal_uid.is_empty() {
            self.append_log("Withdraw order refresh skipped: withdrawal uid is empty");
            return;
        }
        if let Some(client) = self.selected_client() {
            self.append_log(format!("{}: pending withdraw order refresh requested", client.name));
        }
        self.send_command_to_selected(ConsoleCommand::FetchPendingWithdrawOrders {
            withdrawal_uid,
            page_size: 50,
        });
    }

    pub(crate) fn review_withdraw_orders_for_selected_client(&mut self, approve: bool) {
        let mut trade_nos = self.selected_withdraw_trade_nos.iter().cloned().collect::<Vec<_>>();
        trade_nos.sort();
        if trade_nos.is_empty() {
            trade_nos = self
                .withdraw_review_trade_nos
                .split(|ch: char| ch == ',' || ch == '\n' || ch == '\r' || ch.is_whitespace())
                .map(str::trim)
                .filter(|trade_no| !trade_no.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
        }
        if trade_nos.is_empty() {
            self.append_log("No withdraw trade_no entered");
            return;
        }
        if let Some(client) = self.selected_client() {
            let action = if approve { "approve" } else { "reject" };
            self.append_log(format!(
                "{}: {action} {} withdraw orders requested",
                client.name,
                trade_nos.len()
            ));
        }
        self.send_command_to_selected(ConsoleCommand::ReviewWithdrawOrders { trade_nos, approve });
    }

    pub(crate) fn refresh_account_addresses_for_selected_client(&mut self) {
        let wallet_address = self.transfer_form.sub_wallet_address.trim().to_string();
        let chain_code = self.transfer_form.chain_code.trim().to_string();
        self.refresh_account_addresses_for_wallet(wallet_address, chain_code);
    }

    pub(crate) fn refresh_account_addresses_for_wallet(
        &mut self,
        wallet_address: String,
        chain_code: String,
    ) {
        if wallet_address.is_empty() {
            self.account_address_status =
                "Enter a wallet address before showing account addresses.".to_string();
            self.append_log("Account address refresh skipped: wallet address is empty");
            return;
        }
        if chain_code.is_empty() {
            self.account_address_status =
                "Enter a Chain before showing account addresses.".to_string();
            self.append_log("Account address refresh skipped: Chain is empty");
            return;
        }
        if let Some(client) = self.selected_client() {
            self.append_log(format!(
                "{}: account address refresh requested for {} on {}",
                client.name, wallet_address, chain_code
            ));
        }
        self.account_address_status =
            format!("Loading account addresses for {wallet_address} on {chain_code}");
        self.send_command_to_selected(ConsoleCommand::RefreshAccountAddresses {
            wallet_address,
            chain_code,
        });
    }

    pub(crate) fn load_targets_for_selected_client(&mut self) {
        let chain_code = self.transfer_form.chain_code.trim().to_string();
        let sub_wallet_address = self.transfer_form.sub_wallet_address.trim().to_string();
        if let Some(client) = self.selected_client() {
            self.append_log(format!(
                "{}: load targets requested for {} on {}",
                client.name, sub_wallet_address, chain_code
            ));
        }
        self.send_command_to_selected(ConsoleCommand::LoadTransferTargets {
            chain_code,
            sub_wallet_address,
        });
    }

    pub(crate) fn use_account_addresses_as_targets(&mut self) {
        if self.account_address_rows.is_empty() {
            self.append_log("No account addresses to use as transfer targets");
            return;
        }

        self.transfer_form.to_addresses_raw = self
            .account_address_rows
            .iter()
            .map(|row| row.address.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.append_log(format!(
            "Filled {} transfer targets from account addresses",
            self.account_address_rows.len()
        ));
    }

    pub(crate) fn run_transfer_for_selected_client(&mut self) {
        self.show_transfer_confirm = true;
    }

    pub(crate) fn confirm_run_transfer_for_selected_client(&mut self) {
        self.show_transfer_confirm = false;
        let Some(client) = self.selected_client().cloned() else {
            self.append_log("No selected client");
            return;
        };
        let config = match self.transfer_form.build_config(client.password.clone()) {
            Ok(config) => config,
            Err(err) => {
                self.append_log(format!("Invalid transfer config: {err}"));
                return;
            }
        };

        if config.to_addresses.is_empty() {
            self.append_log("No target addresses");
            return;
        }

        self.last_summary = None;
        self.append_log(format!(
            "{}: transfer requested to {} targets",
            client.name,
            config.to_addresses.len()
        ));
        self.send_command_to_selected(ConsoleCommand::RunBatchTransfer { config });
    }

    pub(crate) fn cancel_run_transfer(&mut self) {
        self.show_transfer_confirm = false;
        self.append_log("Transfer cancelled before submit");
    }

    fn send_command_to_selected(&mut self, command: ConsoleCommand) {
        let Some(client) = self.selected_client().cloned() else {
            self.append_log("No selected client");
            return;
        };
        let Some(command_tx) = client.command_tx else {
            self.append_log(format!("{} is not started", client.name));
            return;
        };

        self.is_busy = true;
        if let Err(err) = command_tx.send(command) {
            self.append_log(format!("{} command send failed: {err}", client.name));
            self.is_busy = false;
        }
    }

    pub(crate) fn add_client(&mut self, name: String, config_file: String) {
        let id = ClientId(self.clients.iter().map(|c| c.id.0).max().map(|m| m + 1).unwrap_or(0));
        let client = ClientState::new(id.0, &name, &config_file);
        self.clients.push(client);
        self.transfer_forms.push(TransferForm::for_client(&name));
        self.bind_forms.push(BindForm::for_client(&name));
        self.log_panel_widths.push(340.0);
        self.append_log(format!("Added client: {name} ({config_file})"));
        self.selected_client = id;
    }

    pub(crate) fn remove_client(&mut self, client_id: ClientId) {
        if self.clients.len() <= 1 {
            self.append_log("Cannot remove the last client");
            return;
        }
        let idx = self.clients.iter().position(|c| c.id == client_id).unwrap_or(0);
        self.clients.remove(idx);
        self.transfer_forms.remove(idx);
        self.bind_forms.remove(idx);
        self.log_panel_widths.remove(idx);
        if self.selected_client == client_id {
            self.selected_client = self.clients.first().map(|c| c.id).unwrap_or(ClientId(0));
            self.selected_client_mut().map(|c| {
                c.status = ClientStatus::Idle;
                c.command_tx = None;
            });
        }
        self.append_log(format!("Removed client id={}", client_id.0));
    }

    fn pull_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: ConsoleEvent) {
        match event {
            ConsoleEvent::ClientStarted { client_id } => {
                let mut client_name = None;
                if let Some(client) = self.clients.iter_mut().find(|client| client.id == client_id)
                {
                    client.status = ClientStatus::Ready;
                    client.last_error = None;
                    client_name = Some(client.name.clone());
                }
                if let Some(client_name) = client_name {
                    self.append_log(format!("{client_name}: ready"));
                }
                self.is_busy = false;
                if let Some(client) = self.clients.iter().find(|client| client.id == client_id) {
                    if let Some(command_tx) = client.command_tx.as_ref() {
                        let _ = command_tx.send(ConsoleCommand::RefreshWallets);
                    }
                }
            }
            ConsoleEvent::ClientFailed { client_id, error } => {
                let mut client_name = None;
                if let Some(client) = self.clients.iter_mut().find(|client| client.id == client_id)
                {
                    client.status = ClientStatus::Failed;
                    client.last_error = Some(error.clone());
                    client_name = Some(client.name.clone());
                }
                if let Some(client_name) = client_name {
                    self.append_log(format!("{client_name}: failed - {error}"));
                } else {
                    self.append_log(format!("failed - {error}"));
                }
                self.is_busy = false;
            }
            ConsoleEvent::Log { client_id, message } => {
                let prefix = client_id
                    .and_then(|id| self.clients.iter().find(|client| client.id == id))
                    .map(|client| client.name.as_str())
                    .unwrap_or("console");
                self.append_log(format!("{prefix}: {message}"));
            }
            ConsoleEvent::Notify { client_id, payload } => {
                let prefix = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.as_str())
                    .unwrap_or("client");
                self.append_log(format!("{prefix} notify: {payload}"));
            }
            ConsoleEvent::ImportFinished { client_id, messages } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                for message in messages {
                    self.append_log(format!("{client_name}: {message}"));
                }
                self.is_busy = false;
                if let Some(client) = self.clients.iter().find(|client| client.id == client_id) {
                    if let Some(command_tx) = client.command_tx.as_ref() {
                        let _ = command_tx.send(ConsoleCommand::RefreshWallets);
                    }
                }
            }
            ConsoleEvent::RuntimeInfoLoaded { client_id, info } => {
                let mut client_name = None;
                if let Some(client) = self.clients.iter_mut().find(|client| client.id == client_id)
                {
                    client.runtime_info = Some(info);
                    client_name = Some(client.name.clone());
                }
                if let Some(client_name) = client_name {
                    self.append_log(format!("{client_name}: runtime info loaded"));
                }
            }
            ConsoleEvent::WalletsLoaded { client_id, wallets } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                if self.selected_client == client_id {
                    self.api_wallet_rows = wallets.clone();
                    self.account_address_rows.retain(|row| {
                        wallets.iter().any(|wallet| wallet.address == row.wallet_address)
                    });
                    self.balance_asset_rows.retain(|row| {
                        wallets.iter().any(|wallet| wallet.address == row.wallet_address)
                    });
                    if self.transfer_form.sub_wallet_address.trim().is_empty() {
                        if let Some(wallet) =
                            wallets.iter().find(|wallet| wallet.role == "subwallet")
                        {
                            self.transfer_form.sub_wallet_address = wallet.address.clone();
                            if let Some(slot) = self.transfer_forms.get_mut(client_id.0) {
                                *slot = self.transfer_form.clone();
                            }
                        }
                    }
                }
                self.append_log(format!(
                    "{client_name}: loaded {} api wallets from local DB",
                    wallets.len()
                ));
                self.is_busy = false;
            }
            ConsoleEvent::BindFinished { client_id } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!("{client_name}: bind command finished"));
                self.is_busy = false;
            }
            ConsoleEvent::AccountAddressesLoaded { client_id, wallet_address, rows } => {
                let loaded_count = rows.len();
                self.account_address_rows.retain(|row| row.wallet_address != wallet_address);
                self.account_address_rows.extend(rows);
                self.account_addresses_only = self
                    .account_address_rows
                    .iter()
                    .map(|row| row.address.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!(
                    "{client_name}: loaded {} account addresses",
                    loaded_count
                ));
                self.account_address_status = if self.account_address_rows.is_empty() {
                    format!(
                        "No account addresses found. Check selected client, Subwallet, and Chain."
                    )
                } else {
                    format!("Loaded {} account addresses", self.account_address_rows.len())
                };
                self.is_busy = false;
            }
            ConsoleEvent::BalanceAssetsLoaded { client_id, wallet_address, rows } => {
                let loaded_count = rows.len();
                self.balance_asset_rows.retain(|row| row.wallet_address != wallet_address);
                self.balance_asset_rows.extend(rows);
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!("{client_name}: loaded {} balance assets", loaded_count));
                self.is_busy = false;
            }
            ConsoleEvent::WithdrawOrdersLoaded { client_id, rows } => {
                let loaded_count = rows.len();
                self.pending_withdraw_orders = rows;
                self.selected_withdraw_trade_nos.retain(|trade_no| {
                    self.pending_withdraw_orders.iter().any(|order| &order.trade_no == trade_no)
                });
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!(
                    "{client_name}: loaded {} pending withdraw orders",
                    loaded_count
                ));
                self.is_busy = false;
            }
            ConsoleEvent::WithdrawReviewFinished { client_id } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!("{client_name}: withdraw review command finished"));
                self.selected_withdraw_trade_nos.clear();
                self.is_busy = false;
            }
            ConsoleEvent::ChainNodeSet { client_id, chain_code, rpc_url } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!("{client_name}: {chain_code} node switched to {rpc_url}"));
                self.is_busy = false;
            }
            ConsoleEvent::ChainNodeSetFailed { client_id, error } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!("{client_name}: node switch failed - {error}"));
                self.is_busy = false;
            }
            ConsoleEvent::TaskQueueStatusLoaded { client_id, summary } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!(
                    "{client_name}: task queue loaded all={}, pending={}, running={}, failed={}",
                    summary.all_tasks,
                    summary.pending_tasks,
                    summary.running_tasks,
                    summary.failed_task_count
                ));
                self.task_queue_summary = Some(summary);
                self.observe_tab = ObserveTab::Diagnostics;
                self.is_busy = false;
            }
            ConsoleEvent::TradeDiagnosticsLoaded { client_id, diagnostics } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!(
                    "{client_name}: trade {} diagnostics loaded, {} records",
                    diagnostics.trade_no,
                    diagnostics.records.len()
                ));
                self.trade_diagnostics = Some(diagnostics);
                self.observe_tab = ObserveTab::Diagnostics;
                self.is_busy = false;
            }
            ConsoleEvent::LoadedTargets { client_id, targets } => {
                self.transfer_form.to_addresses_raw = targets.join("\n");
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!(
                    "{client_name}: loaded {} targets",
                    self.transfer_form.to_addresses_raw.lines().count()
                ));
                self.is_busy = false;
            }
            ConsoleEvent::TransferFinished { client_id, summary } => {
                let client_name = self
                    .clients
                    .iter()
                    .find(|client| client.id == client_id)
                    .map(|client| client.name.clone())
                    .unwrap_or_else(|| "client".to_string());
                self.append_log(format!(
                    "{client_name}: transfer done total={}, success={}, failed={}",
                    summary.total, summary.success, summary.failed
                ));
                self.last_summary = Some(summary);
                self.is_busy = false;
            }
        }
    }
}

impl eframe::App for ApiWalletConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pull_events();
        ui::draw(self, ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn discover_clients() -> Vec<ClientState> {
    let search_paths: Vec<std::path::PathBuf> = {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut paths = Vec::new();
        paths.push(cwd.join("examples/client_config"));
        paths.push(cwd.join("wallet-api/examples/client_config"));
        paths.push(cwd.join("../examples/client_config"));
        paths.push(cwd.join("../wallet-api/examples/client_config"));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                paths.push(parent.join("examples/client_config"));
                if let Some(grandparent) = parent.parent() {
                    paths.push(grandparent.join("examples/client_config"));
                }
            }
        }
        if let Ok(dir) = std::env::var("API_WALLET_CONSOLE_CONFIG_DIR") {
            paths.push(std::path::PathBuf::from(dir));
        }
        paths
    };

    let mut clients = Vec::new();

    for config_dir in &search_paths {
        if let Ok(entries) = std::fs::read_dir(config_dir) {
            let mut toml_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
                .collect();
            if toml_files.is_empty() {
                continue;
            }
            toml_files.sort_by_key(|e| e.file_name());

            for (id, entry) in toml_files.iter().enumerate() {
                let path = entry.path();
                let stem =
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
                let file_name = entry.file_name();
                let config_file = file_name.to_str().unwrap_or("").to_string();
                clients.push(ClientState::new(id as usize, &stem, &config_file));
            }
            break;
        }
    }

    if clients.is_empty() {
        clients.push(ClientState::new(0, "client1", "client1.toml"));
    }

    clients
}
