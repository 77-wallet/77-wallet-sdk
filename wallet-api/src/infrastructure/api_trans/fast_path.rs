use tracing::{info, warn};

#[derive(Debug, Clone, Copy)]
pub(crate) enum FastPathFlow {
    Collect,
    Withdraw,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FastPathReason {
    TxAckSent,
    FeeEstimatePersisted,
    LocalDelegationPersisted,
    ResourceGateReleased,
    TxFeeResAckSent,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FastPathTarget {
    EstimateFee,
    EvalResourceGate,
    ExecuteLocalResourceDelegation,
    BuildTx,
    BroadcastTx,
}

impl FastPathFlow {
    fn as_str(self) -> &'static str {
        match self {
            Self::Collect => "collect",
            Self::Withdraw => "withdraw",
        }
    }
}

impl FastPathReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::TxAckSent => "tx_ack_sent",
            Self::FeeEstimatePersisted => "fee_estimate_persisted",
            Self::LocalDelegationPersisted => "local_delegation_persisted",
            Self::ResourceGateReleased => "resource_gate_released",
            Self::TxFeeResAckSent => "tx_fee_res_ack_sent",
        }
    }
}

impl FastPathTarget {
    fn as_str(self) -> &'static str {
        match self {
            Self::EstimateFee => "estimate_fee",
            Self::EvalResourceGate => "eval_resource_gate",
            Self::ExecuteLocalResourceDelegation => "execute_local_resource_delegation",
            Self::BuildTx => "build_tx",
            Self::BroadcastTx => "broadcast_tx",
        }
    }
}

pub(crate) fn dispatch_fast_path<D, E, F>(
    flow: FastPathFlow,
    reason: FastPathReason,
    target: FastPathTarget,
    trade_no: &str,
    dispatch: D,
    on_dispatch_failed: F,
) where
    D: FnOnce() -> Result<(), E>,
    E: std::fmt::Display,
    F: FnOnce(),
{
    info!(
        trade_no = %trade_no,
        flow = flow.as_str(),
        fast_path = true,
        fast_path_reason = reason.as_str(),
        fast_path_target = target.as_str(),
        "Triggering transaction fast path"
    );

    if let Err(error) = dispatch() {
        warn!(
            trade_no = %trade_no,
            flow = flow.as_str(),
            fast_path = true,
            fast_path_reason = reason.as_str(),
            fast_path_target = target.as_str(),
            error = %error,
            "Failed to dispatch transaction fast path"
        );
        on_dispatch_failed();
    }
}
