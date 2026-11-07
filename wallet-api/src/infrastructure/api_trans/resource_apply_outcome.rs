#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlatformApplyOutcome {
    AcceptedWithResourceTradeNo(String),
    AcceptedWithOriginalTradeNo,
    Rejected,
}

impl PlatformApplyOutcome {
    pub(crate) fn from_backend_response(
        is_success: bool,
        dl_trade_no: Option<String>,
    ) -> PlatformApplyOutcome {
        if !is_success {
            return PlatformApplyOutcome::Rejected;
        }

        match dl_trade_no {
            Some(trade_no) if !trade_no.trim().is_empty() => {
                PlatformApplyOutcome::AcceptedWithResourceTradeNo(trade_no.trim().to_string())
            }
            _ => PlatformApplyOutcome::AcceptedWithOriginalTradeNo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformApplyOutcome;

    #[test]
    fn platform_apply_success_accepts_resource_trade_no() {
        assert_eq!(
            PlatformApplyOutcome::from_backend_response(true, Some("DL_1".to_string())),
            PlatformApplyOutcome::AcceptedWithResourceTradeNo("DL_1".to_string())
        );
        assert_eq!(
            PlatformApplyOutcome::from_backend_response(true, Some(" DL_1 ".to_string())),
            PlatformApplyOutcome::AcceptedWithResourceTradeNo("DL_1".to_string())
        );
    }

    #[test]
    fn platform_apply_success_without_resource_trade_no_waits_original_order_result() {
        assert_eq!(
            PlatformApplyOutcome::from_backend_response(true, None),
            PlatformApplyOutcome::AcceptedWithOriginalTradeNo
        );
        assert_eq!(
            PlatformApplyOutcome::from_backend_response(true, Some("  ".to_string())),
            PlatformApplyOutcome::AcceptedWithOriginalTradeNo
        );
    }

    #[test]
    fn platform_apply_failure_is_rejected() {
        assert_eq!(
            PlatformApplyOutcome::from_backend_response(false, Some("DL_1".to_string())),
            PlatformApplyOutcome::Rejected
        );
    }
}
