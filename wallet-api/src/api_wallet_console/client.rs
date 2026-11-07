use tokio::sync::mpsc::UnboundedSender;

use super::{commands::ConsoleCommand, events::ClientRuntimeInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientStatus {
    Idle,
    Starting,
    Ready,
    Failed,
}

impl ClientStatus {
    pub fn label(&self) -> &'static str {
        match self {
            ClientStatus::Idle => "未启动",
            ClientStatus::Starting => "启动中",
            ClientStatus::Ready => "就绪",
            ClientStatus::Failed => "失败",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientState {
    pub id: ClientId,
    pub name: String,
    pub config_file: String,
    pub password: String,
    pub status: ClientStatus,
    pub last_error: Option<String>,
    pub runtime_info: Option<ClientRuntimeInfo>,
    pub command_tx: Option<UnboundedSender<ConsoleCommand>>,
}

impl ClientState {
    pub fn new(id: usize, name: &str, config_file: &str) -> Self {
        Self {
            id: ClientId(id),
            name: name.to_string(),
            config_file: config_file.to_string(),
            password: "q1111111".to_string(),
            status: ClientStatus::Idle,
            last_error: None,
            runtime_info: None,
            command_tx: None,
        }
    }
}
