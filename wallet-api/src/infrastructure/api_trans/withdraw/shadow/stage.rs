#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdvancementPoint {
    NeedTxAck,
    NeedResourceGate,
    NeedPlatformDelegateApply,
    CanBuild,
    CanBroadcast,
    NeedRecover,
    NeedTxExecReceiptUpload,
    NeedTxResAck,
    FullyBlocked,
}

/// 推进点顺序常量
/// - 顺序与 scan_round 完全一致
/// - try_advance 必须使用此常量，确保行为一致性
pub const ADVANCEMENT_ORDER: &[AdvancementPoint] = &[
    AdvancementPoint::NeedTxAck,
    AdvancementPoint::NeedResourceGate,
    AdvancementPoint::NeedPlatformDelegateApply,
    AdvancementPoint::CanBuild,
    AdvancementPoint::CanBroadcast,
    AdvancementPoint::NeedRecover,
    AdvancementPoint::NeedTxExecReceiptUpload,
    AdvancementPoint::NeedTxResAck,
];

impl AdvancementPoint {
    pub fn base_severity(&self) -> u8 {
        match self {
            AdvancementPoint::NeedTxAck => 1,
            AdvancementPoint::NeedResourceGate => 1,
            AdvancementPoint::NeedPlatformDelegateApply => 1,
            AdvancementPoint::CanBuild => 0,
            AdvancementPoint::CanBroadcast => 0,
            AdvancementPoint::NeedRecover => 2,
            AdvancementPoint::NeedTxExecReceiptUpload => 1,
            AdvancementPoint::NeedTxResAck => 1,
            AdvancementPoint::FullyBlocked => 4,
        }
    }

    pub fn wait_threshold_minutes(&self) -> i64 {
        match self {
            AdvancementPoint::NeedTxAck => 2,
            AdvancementPoint::NeedResourceGate => 2,
            AdvancementPoint::NeedPlatformDelegateApply => 2,
            AdvancementPoint::CanBuild => 5,
            AdvancementPoint::CanBroadcast => 5,
            AdvancementPoint::NeedRecover => 10,
            AdvancementPoint::NeedTxExecReceiptUpload => 15,
            AdvancementPoint::NeedTxResAck => 8,
            AdvancementPoint::FullyBlocked => 1,
        }
    }

    pub fn next_expected_fact(&self) -> &'static str {
        match self {
            AdvancementPoint::NeedTxAck => "tx_ack_sent_at",
            AdvancementPoint::NeedResourceGate => "resource_gate_released_at",
            AdvancementPoint::NeedPlatformDelegateApply => "resource_gate_result",
            AdvancementPoint::CanBuild => "raw_tx",
            AdvancementPoint::CanBroadcast => "last_broadcast_at",
            AdvancementPoint::NeedRecover => "transaction_time",
            AdvancementPoint::NeedTxExecReceiptUpload => "tx_exec_receipt_uploaded_at",
            AdvancementPoint::NeedTxResAck => "tx_res_ack_sent_at",
            AdvancementPoint::FullyBlocked => "finished_at",
        }
    }
}
