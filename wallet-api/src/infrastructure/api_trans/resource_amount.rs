use crate::error::{
    business::api_wallet::{ApiWalletError, trans::TransError},
    service::ServiceError,
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlatformResourceApplyAmounts {
    pub(crate) resource_amount: f64,
    pub(crate) native_token_amount: i64,
}

pub(crate) fn energy_shortfall_to_apply_amounts(
    amount: &str,
    energy_per_trx: f64,
) -> Result<PlatformResourceApplyAmounts, ServiceError> {
    let resource_amount: f64 = amount.parse().map_err(|e| {
        ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
            ApiWalletError::Trans(TransError::BuildWithdrawTransactionFailed(format!(
                "Invalid delegation amount: {}",
                e
            ))),
        ))
    })?;

    if resource_amount <= 0.0 {
        return Err(ServiceError::Parameter("delegation resource amount must be positive".into()));
    }
    if energy_per_trx <= 0.0 {
        return Err(ServiceError::Parameter("TRON energy price must be positive".into()));
    }

    Ok(PlatformResourceApplyAmounts {
        resource_amount,
        native_token_amount: (resource_amount / energy_per_trx).ceil() as i64,
    })
}

pub(crate) fn parse_resource_delegation_native_trx_units(
    amount: &str,
) -> Result<i64, ServiceError> {
    let parsed = Decimal::from_str(amount.trim()).map_err(|_| {
        ServiceError::Parameter(format!("invalid resource delegation native amount: {amount}"))
    })?;
    if parsed <= Decimal::ZERO {
        return Err(ServiceError::Parameter(format!(
            "resource delegation native amount must be positive: {amount}"
        )));
    }

    // The backend field is a TRX amount and may contain decimals. The current
    // TRON stake adapter accepts whole TRX units and multiplies by SUN
    // internally, so round up to avoid under-delegating resources.
    parsed.ceil().to_i64().ok_or_else(|| {
        ServiceError::Parameter(format!("resource delegation native amount is too large: {amount}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{energy_shortfall_to_apply_amounts, parse_resource_delegation_native_trx_units};

    #[test]
    fn energy_shortfall_to_apply_amounts_uses_chain_energy_price() {
        let amounts = energy_shortfall_to_apply_amounts("800", 400.0).expect("amounts");

        assert_eq!(amounts.resource_amount, 800.0);
        assert_eq!(amounts.native_token_amount, 2);
    }

    #[test]
    fn energy_shortfall_to_apply_amounts_rounds_native_amount_up_to_whole_trx() {
        let amounts = energy_shortfall_to_apply_amounts("14650", 74.5807).expect("amounts");

        assert_eq!(amounts.resource_amount, 14650.0);
        assert_eq!(amounts.native_token_amount, 197);
    }

    #[test]
    fn energy_shortfall_to_apply_amounts_rejects_zero_energy_price() {
        assert!(energy_shortfall_to_apply_amounts("800", 0.0).is_err());
    }

    #[test]
    fn parse_resource_delegation_native_trx_units_accepts_decimal_backend_value() {
        let amount =
            parse_resource_delegation_native_trx_units("196.4287194491667").expect("parse amount");

        assert_eq!(amount, 197);
    }

    #[test]
    fn parse_resource_delegation_native_trx_units_rejects_non_positive_amount() {
        assert!(parse_resource_delegation_native_trx_units("0").is_err());
    }
}
