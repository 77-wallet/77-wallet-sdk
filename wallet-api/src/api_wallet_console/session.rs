use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

use super::{
    client::ClientState, commands::ConsoleCommand, events::ConsoleEvent,
    worker_protocol::WorkerEvent,
};

pub fn spawn_client_session(
    client: ClientState,
    mut command_rx: tokio::sync::mpsc::UnboundedReceiver<ConsoleCommand>,
    event_tx: mpsc::Sender<ConsoleEvent>,
) {
    thread::spawn(move || {
        let client_id = client.id;
        let mut command = worker_command(&client.config_file, &client.password);
        let mut child = match command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                let _ =
                    event_tx.send(ConsoleEvent::ClientFailed { client_id, error: err.to_string() });
                return;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            let tx = event_tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let clean_line = strip_ansi_codes(&line);
                    match serde_json::from_str::<WorkerEvent>(&clean_line) {
                        Ok(event) => forward_worker_event(client_id, event, &tx),
                        Err(_) => {
                            let _ = tx.send(ConsoleEvent::Log {
                                client_id: Some(client_id),
                                message: format!("worker stdout: {clean_line}"),
                            });
                        }
                    }
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let tx = event_tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let clean_line = strip_ansi_codes(&line);
                    let trimmed = clean_line.trim_start();
                    if trimmed.starts_with("Compiling ")
                        || trimmed.starts_with("Checking ")
                        || trimmed.starts_with("Finished ")
                        || trimmed.starts_with("Running ")
                        || trimmed.starts_with("warning:")
                        || trimmed.starts_with("error:")
                        || trimmed.starts_with("-->")
                        || trimmed.starts_with("|")
                        || trimmed.starts_with("=")
                        || trimmed.starts_with("help:")
                        || trimmed.starts_with("`wallet-")
                        || trimmed.contains("generated 5 warnings")
                        || trimmed.contains("let header_count")
                        || trimmed.contains("^^^^^^^^")
                    {
                        continue;
                    }
                    let _ = tx.send(ConsoleEvent::Log {
                        client_id: Some(client_id),
                        message: format!("worker stderr: {clean_line}"),
                    });
                }
            });
        }

        let Some(mut stdin) = child.stdin.take() else {
            let _ = event_tx.send(ConsoleEvent::ClientFailed {
                client_id,
                error: "worker stdin unavailable".to_string(),
            });
            return;
        };

        while let Some(command) = command_rx.blocking_recv() {
            let line = match serde_json::to_string(&command) {
                Ok(line) => line,
                Err(err) => {
                    let _ = event_tx.send(ConsoleEvent::Log {
                        client_id: Some(client_id),
                        message: format!("command serialize failed: {err}"),
                    });
                    continue;
                }
            };
            if writeln!(stdin, "{line}").and_then(|_| stdin.flush()).is_err() {
                let _ = event_tx.send(ConsoleEvent::ClientFailed {
                    client_id,
                    error: "worker command pipe closed".to_string(),
                });
                break;
            }
        }
    });
}

fn forward_worker_event(
    client_id: super::client::ClientId,
    event: WorkerEvent,
    tx: &mpsc::Sender<ConsoleEvent>,
) {
    let event = match event {
        WorkerEvent::Started => ConsoleEvent::ClientStarted { client_id },
        WorkerEvent::Failed { error } => ConsoleEvent::ClientFailed { client_id, error },
        WorkerEvent::Log { message } => ConsoleEvent::Log { client_id: Some(client_id), message },
        WorkerEvent::Notify { payload } => ConsoleEvent::Notify { client_id, payload },
        WorkerEvent::ImportFinished { messages } => {
            ConsoleEvent::ImportFinished { client_id, messages }
        }
        WorkerEvent::RuntimeInfoLoaded { info } => {
            ConsoleEvent::RuntimeInfoLoaded { client_id, info }
        }
        WorkerEvent::WalletsLoaded { wallets } => {
            ConsoleEvent::WalletsLoaded { client_id, wallets }
        }
        WorkerEvent::BindFinished => ConsoleEvent::BindFinished { client_id },
        WorkerEvent::AccountAddressesLoaded { wallet_address, rows } => {
            ConsoleEvent::AccountAddressesLoaded { client_id, wallet_address, rows }
        }
        WorkerEvent::BalanceAssetsLoaded { wallet_address, rows } => {
            ConsoleEvent::BalanceAssetsLoaded { client_id, wallet_address, rows }
        }
        WorkerEvent::WithdrawOrdersLoaded { rows } => {
            ConsoleEvent::WithdrawOrdersLoaded { client_id, rows }
        }
        WorkerEvent::WithdrawReviewFinished => ConsoleEvent::WithdrawReviewFinished { client_id },
        WorkerEvent::ChainNodeSet { chain_code, rpc_url } => {
            ConsoleEvent::ChainNodeSet { client_id, chain_code, rpc_url }
        }
        WorkerEvent::ChainNodeSetFailed { error } => {
            ConsoleEvent::ChainNodeSetFailed { client_id, error }
        }
        WorkerEvent::TaskQueueStatusLoaded { summary } => {
            ConsoleEvent::TaskQueueStatusLoaded { client_id, summary }
        }
        WorkerEvent::TradeDiagnosticsLoaded { diagnostics } => {
            ConsoleEvent::TradeDiagnosticsLoaded { client_id, diagnostics }
        }
        WorkerEvent::LoadedTargets { targets } => {
            ConsoleEvent::LoadedTargets { client_id, targets }
        }
        WorkerEvent::TransferFinished { summary } => {
            ConsoleEvent::TransferFinished { client_id, summary }
        }
    };
    let _ = tx.send(event);
}

fn worker_command(config_file: &str, password: &str) -> Command {
    if std::env::var("API_WALLET_CONSOLE_DIRECT_WORKER").as_deref() == Ok("1") {
        if let Ok(path) = worker_path() {
            if path.exists() {
                let mut command = Command::new(path);
                command.arg("--config").arg(config_file).arg("--password").arg(password);
                return command;
            }
        }
    }

    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("-p")
        .arg("wallet-api")
        .arg("--bin")
        .arg("api_wallet_console_worker")
        .arg("--")
        .arg("--config")
        .arg(config_file)
        .arg("--password")
        .arg(password);
    command
}

fn worker_path() -> Result<std::path::PathBuf, String> {
    let mut path = std::env::current_exe().map_err(|err| err.to_string())?;
    path.set_file_name(format!("api_wallet_console_worker{}", std::env::consts::EXE_SUFFIX));
    Ok(path)
}

fn strip_ansi_codes(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}
