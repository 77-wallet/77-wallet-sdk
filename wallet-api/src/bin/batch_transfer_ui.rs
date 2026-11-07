use std::{sync::mpsc, thread};

use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea};
use wallet_api::{
    batch_transfer::{
        BatchTransferConfig, BatchTransferResultItem, BatchTransferSummary,
        collect_target_addresses, parse_u8, parse_u64, parse_usize, run_batch_transfer,
    },
    testkit::env::get_manager,
};

fn main() {
    let native = eframe::NativeOptions::default();
    let run = eframe::run_native(
        "Batch Transfer Test Tool",
        native,
        Box::new(|_| Ok(Box::new(BatchTransferUiApp::new()))),
    );

    if let Err(err) = run {
        eprintln!("failed to start app: {err}");
    }
}

#[derive(Clone)]
struct AppForm {
    chain_code: String,
    from_address: String,
    sub_wallet_address: String,
    value: String,
    symbol: String,
    decimals: String,
    max_in_flight: String,
    start_interval_ms: String,
    password: String,
    fee_setting: String,
    to_addresses_raw: String,
}

impl Default for AppForm {
    fn default() -> Self {
        Self {
            chain_code: "tron".to_string(),
            from_address: "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5".to_string(),
            sub_wallet_address: "0x5489c657Be2504D657f1F56AB04abfE3C77ceC34".to_string(),
            value: "5".to_string(),
            symbol: "TRX".to_string(),
            decimals: "6".to_string(),
            max_in_flight: "3".to_string(),
            start_interval_ms: "300".to_string(),
            password: "q1111111".to_string(),
            fee_setting: "".to_string(),
            to_addresses_raw: String::new(),
        }
    }
}

struct RunState {
    summary: Option<BatchTransferSummary>,
    log_lines: Vec<String>,
    is_running: bool,
    receiver: Option<mpsc::Receiver<UiEvent>>,
}

struct BatchTransferUiApp {
    form: AppForm,
    run_state: RunState,
}

impl BatchTransferUiApp {
    fn new() -> Self {
        Self {
            form: AppForm::default(),
            run_state: RunState {
                summary: None,
                log_lines: vec![],
                is_running: false,
                receiver: None,
            },
        }
    }

    fn append_line(&mut self, text: impl Into<String>) {
        self.run_state.log_lines.push(text.into());
        if self.run_state.log_lines.len() > 200 {
            self.run_state.log_lines.drain(0..self.run_state.log_lines.len() - 200);
        }
    }

    fn push_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Log(line) => self.append_line(line),
            UiEvent::LoadedTargets(targets) => {
                self.form.to_addresses_raw = targets.join("\n");
                self.append_line(format!(
                    "Loaded {} target addresses",
                    self.form.to_addresses_raw.lines().count()
                ));
            }
            UiEvent::Done(result) => {
                self.run_state.summary = Some(result.clone());
                self.run_state.is_running = false;
                self.run_state.receiver = None;
                self.run_state.log_lines.push(format!(
                    "Done. total={}, success={}, failed={}",
                    result.total, result.success, result.failed
                ));
            }
        }
    }

    fn pull_events(&mut self) {
        loop {
            let event = if let Some(rx) = self.run_state.receiver.as_mut() {
                rx.try_recv().ok()
            } else {
                None
            };

            if let Some(event) = event {
                self.push_event(event);
            } else {
                break;
            }
        }
    }

    fn build_config(&self) -> anyhow::Result<BatchTransferConfig> {
        let decimals = parse_u8(&self.form.decimals, 6)?;
        let max_in_flight = parse_usize(&self.form.max_in_flight, 3)?;
        let start_interval_ms = parse_u64(&self.form.start_interval_ms, 300)?;
        let to_addresses = self
            .form
            .to_addresses_raw
            .lines()
            .map(str::trim)
            .filter(|addr| !addr.is_empty())
            .map(ToString::to_string)
            .collect();

        Ok(BatchTransferConfig {
            chain_code: self.form.chain_code.trim().to_string(),
            from_address: self.form.from_address.trim().to_string(),
            to_addresses,
            value: self.form.value.trim().to_string(),
            token_symbol: self.form.symbol.trim().to_string(),
            token_decimals: decimals,
            max_in_flight,
            start_interval_ms,
            password: self.form.password.clone(),
            fee_setting: self.form.fee_setting.clone(),
        })
    }

    fn update_targets_from_subwallet(&mut self) {
        let chain_code = self.form.chain_code.trim().to_string();
        let sub_wallet_address = self.form.sub_wallet_address.trim().to_string();
        let password = self.form.password.clone();
        let (tx, rx) = mpsc::channel();
        self.run_state.is_running = true;
        self.run_state.receiver = Some(rx);
        self.append_line(format!("loading subwallet {} on {}", sub_wallet_address, chain_code));

        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new();
            if let Err(err) = rt {
                let _ = tx.send(UiEvent::Log(format!("runtime init failed: {err}")));
                let _ = tx.send(UiEvent::Done(BatchTransferSummary::default()));
                return;
            }
            let rt = rt.unwrap();

            let result = rt.block_on(async {
                let (wallet_manager, _test_params) = get_manager().await?;
                wallet_manager.init_api_swap().await?;
                let _ = wallet_manager.set_passwd_cache(&password).await;
                let subaccounts = wallet_manager
                    .list_api_wallet_account(
                        &sub_wallet_address,
                        None,
                        Some(chain_code.clone()),
                        0,
                        500,
                    )
                    .await?;
                let addrs = collect_target_addresses(subaccounts.data, &chain_code);
                Ok::<Vec<String>, anyhow::Error>(addrs)
            });

            match result {
                Ok(targets) => {
                    let _ = tx.send(UiEvent::LoadedTargets(targets));
                }
                Err(err) => {
                    let _ = tx.send(UiEvent::Log(format!("load subwallet failed: {err}")));
                }
            }
            let _ = tx.send(UiEvent::Done(BatchTransferSummary::default()));
        });
    }

    fn start_transfer(&mut self) {
        let cfg = match self.build_config() {
            Ok(cfg) => cfg,
            Err(err) => {
                self.append_line(format!("invalid config: {err}"));
                return;
            }
        };

        if cfg.to_addresses.is_empty() {
            self.append_line(
                "no target addresses. Fill Target addresses or click Load subwallet first.",
            );
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.run_state.is_running = true;
        self.run_state.summary = None;
        self.run_state.log_lines.clear();
        self.run_state.receiver = Some(rx);

        thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(err) => {
                    let _ = tx.send(UiEvent::Log(format!("runtime init failed: {err}")));
                    let mut summary = BatchTransferSummary::default();
                    summary.logs.push(BatchTransferResultItem {
                        to_address: "runtime".to_string(),
                        success: false,
                        detail: err.to_string(),
                    });
                    let _ = tx.send(UiEvent::Done(summary));
                    return;
                }
            };

            let result: Result<BatchTransferSummary, String> = rt.block_on(async {
                let (wallet_manager, _test_params) =
                    get_manager().await.map_err(|err| err.to_string())?;
                wallet_manager.init_api_swap().await.map_err(|err| err.to_string())?;
                let _ = wallet_manager
                    .set_passwd_cache(&cfg.password)
                    .await
                    .map_err(|err| err.to_string())?;

                let summary = run_batch_transfer(wallet_manager.clone(), &cfg)
                    .await
                    .map_err(|err| err.to_string())?;
                for item in summary.logs.iter() {
                    let _ = tx.send(UiEvent::Log(format!(
                        "{} {} - {}",
                        item.to_address,
                        if item.success { "OK" } else { "FAILED" },
                        item.detail
                    )));
                }
                Ok(summary)
            });

            let summary = match result {
                Ok(summary) => summary,
                Err(err) => {
                    let mut summary = BatchTransferSummary::default();
                    summary.logs.push(BatchTransferResultItem {
                        to_address: "batch".to_string(),
                        success: false,
                        detail: err,
                    });
                    summary
                }
            };
            let _ = tx.send(UiEvent::Done(summary));
        });
    }
}

impl eframe::App for BatchTransferUiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pull_events();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Wallet API Batch Transfer");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Chain:");
                ui.text_edit_singleline(&mut self.form.chain_code);
            });
            ui.horizontal(|ui| {
                ui.label("From:");
                ui.text_edit_singleline(&mut self.form.from_address);
            });
            ui.horizontal(|ui| {
                ui.label("Subwallet:");
                ui.text_edit_singleline(&mut self.form.sub_wallet_address);
            });
            ui.horizontal(|ui| {
                ui.label("Amount:");
                ui.text_edit_singleline(&mut self.form.value);
            });
            ui.horizontal(|ui| {
                ui.label("Symbol:");
                ui.text_edit_singleline(&mut self.form.symbol);
            });
            ui.horizontal(|ui| {
                ui.label("Decimals:");
                ui.text_edit_singleline(&mut self.form.decimals);
            });
            ui.horizontal(|ui| {
                ui.label("Concurrency:");
                ui.text_edit_singleline(&mut self.form.max_in_flight);
                ui.label("Interval ms:");
                ui.text_edit_singleline(&mut self.form.start_interval_ms);
            });
            ui.horizontal(|ui| {
                ui.label("Password:");
                ui.text_edit_singleline(&mut self.form.password);
            });
            ui.horizontal(|ui| {
                ui.label("Fee setting:");
                ui.text_edit_singleline(&mut self.form.fee_setting);
            });

            ui.add_space(6.0);
            ui.label("Target addresses, one per line:");
            ui.text_edit_multiline(&mut self.form.to_addresses_raw);

            ui.add_space(8.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.run_state.is_running,
                        egui::Button::new("Load targets from subwallet"),
                    )
                    .clicked()
                {
                    self.update_targets_from_subwallet();
                }
                if ui.add_enabled(!self.run_state.is_running, egui::Button::new("Start")).clicked()
                {
                    self.start_transfer();
                }
            });

            ui.separator();
            if self.run_state.is_running {
                ui.colored_label(Color32::YELLOW, "Running...");
            }

            ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for line in &self.run_state.log_lines {
                    ui.label(line);
                }
            });

            if let Some(summary) = &self.run_state.summary {
                ui.separator();
                ui.label(RichText::new(format!(
                    "Summary: total={}, success={}, failed={}",
                    summary.total, summary.success, summary.failed
                )));
            }
        });
    }
}

#[derive(Clone, Debug)]
enum UiEvent {
    Log(String),
    LoadedTargets(Vec<String>),
    Done(BatchTransferSummary),
}
