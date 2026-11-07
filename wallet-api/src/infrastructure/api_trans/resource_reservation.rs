use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResourceBudget {
    pub(crate) energy: u64,
    pub(crate) bandwidth: u64,
}

impl ResourceBudget {
    pub(crate) fn new(energy: u64, bandwidth: u64) -> Self {
        Self { energy, bandwidth }
    }

    fn saturating_sub(self, other: Self) -> Self {
        Self {
            energy: self.energy.saturating_sub(other.energy),
            bandwidth: self.bandwidth.saturating_sub(other.bandwidth),
        }
    }

    fn component_max(self, other: Self) -> Self {
        Self {
            energy: self.energy.max(other.energy),
            bandwidth: self.bandwidth.max(other.bandwidth),
        }
    }

    fn add_assign(&mut self, other: Self) {
        self.energy = self.energy.saturating_add(other.energy);
        self.bandwidth = self.bandwidth.saturating_add(other.bandwidth);
    }

    fn sub_assign_saturating(&mut self, other: Self) {
        self.energy = self.energy.saturating_sub(other.energy);
        self.bandwidth = self.bandwidth.saturating_sub(other.bandwidth);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResourceShortfall {
    pub(crate) energy: u64,
    pub(crate) bandwidth: u64,
    pub(crate) effective_available: ResourceBudget,
    pub(crate) reserved: ResourceBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResourceReservationDecision {
    Reserved { effective_available: ResourceBudget, reserved: ResourceBudget },
    NeedMore(ResourceShortfall),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FeeReservationDecision {
    Reserved { total_reserved: u64 },
    Insufficient { available: u64, reserved: u64, required: u64 },
}

#[derive(Debug, Default)]
struct FeeReservationState {
    totals: HashMap<String, u64>,
    by_trade_no: HashMap<String, (String, u64)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CollectFeeReservationManager {
    inner: Arc<Mutex<FeeReservationState>>,
}

impl CollectFeeReservationManager {
    pub(crate) fn reserve_if_available(
        &self,
        funding_key: impl Into<String>,
        trade_no: impl Into<String>,
        required: u64,
        available: u64,
    ) -> FeeReservationDecision {
        let funding_key = funding_key.into();
        let trade_no = trade_no.into();
        let mut state = self.inner.lock().expect("fee reservation lock poisoned");

        if let Some((existing_key, _)) = state.by_trade_no.get(&trade_no).cloned() {
            if existing_key == funding_key {
                let total_reserved = state.totals.get(&funding_key).copied().unwrap_or_default();
                return FeeReservationDecision::Reserved { total_reserved };
            }
            Self::release_locked(&mut state, &trade_no);
        }

        let reserved = state.totals.get(&funding_key).copied().unwrap_or_default();
        if available.saturating_sub(reserved) < required {
            return FeeReservationDecision::Insufficient { available, reserved, required };
        }

        state.totals.insert(funding_key.clone(), reserved.saturating_add(required));
        state.by_trade_no.insert(trade_no, (funding_key, required));
        FeeReservationDecision::Reserved { total_reserved: reserved.saturating_add(required) }
    }

    pub(crate) fn release(&self, trade_no: &str) {
        let mut state = self.inner.lock().expect("fee reservation lock poisoned");
        Self::release_locked(&mut state, trade_no);
    }

    fn release_locked(state: &mut FeeReservationState, trade_no: &str) {
        let Some((key, amount)) = state.by_trade_no.remove(trade_no) else {
            return;
        };
        if let Some(total) = state.totals.get_mut(&key) {
            *total = total.saturating_sub(amount);
            if *total == 0 {
                state.totals.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod collect_fee_reservation_tests {
    use super::*;

    #[test]
    fn same_funding_address_reserves_idempotently_and_releases() {
        let manager = CollectFeeReservationManager::default();
        assert_eq!(
            manager.reserve_if_available("tron:funding", "C1", 700_000, 1_000_000),
            FeeReservationDecision::Reserved { total_reserved: 700_000 }
        );
        assert_eq!(
            manager.reserve_if_available("tron:funding", "C1", 700_000, 1_000_000),
            FeeReservationDecision::Reserved { total_reserved: 700_000 }
        );
        assert_eq!(
            manager.reserve_if_available("tron:funding", "C2", 400_000, 1_000_000),
            FeeReservationDecision::Insufficient {
                available: 1_000_000,
                reserved: 700_000,
                required: 400_000,
            }
        );

        manager.release("C1");
        assert_eq!(
            manager.reserve_if_available("tron:funding", "C2", 400_000, 1_000_000),
            FeeReservationDecision::Reserved { total_reserved: 400_000 }
        );
    }
}

#[derive(Debug, Default)]
struct ResourceReservationState {
    totals: HashMap<String, ResourceBudget>,
    by_trade_no: HashMap<String, (String, ResourceBudget)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WithdrawResourceReservationManager {
    inner: Arc<Mutex<ResourceReservationState>>,
}

impl WithdrawResourceReservationManager {
    pub(crate) fn restore(
        &self,
        resource_key: impl Into<String>,
        trade_no: impl Into<String>,
        budget: ResourceBudget,
    ) -> bool {
        let resource_key = resource_key.into();
        let trade_no = trade_no.into();
        let mut state = self.inner.lock().expect("resource reservation lock poisoned");
        if state.by_trade_no.contains_key(&trade_no) {
            return false;
        }

        state.totals.entry(resource_key.clone()).or_default().add_assign(budget);
        state.by_trade_no.insert(trade_no, (resource_key, budget));
        true
    }

    pub(crate) fn reserve_if_available(
        &self,
        resource_key: impl Into<String>,
        trade_no: impl Into<String>,
        required: ResourceBudget,
        available: ResourceBudget,
    ) -> ResourceReservationDecision {
        self.reserve_with_persisted_budget(
            resource_key,
            trade_no,
            required,
            available,
            ResourceBudget::default(),
        )
    }

    pub(crate) fn reserve_with_persisted_budget(
        &self,
        resource_key: impl Into<String>,
        trade_no: impl Into<String>,
        required: ResourceBudget,
        available: ResourceBudget,
        persisted: ResourceBudget,
    ) -> ResourceReservationDecision {
        let resource_key = resource_key.into();
        let trade_no = trade_no.into();
        let mut state = self.inner.lock().expect("resource reservation lock poisoned");
        if let Some((existing_key, existing_budget)) = state.by_trade_no.get(&trade_no).cloned() {
            if existing_key == resource_key {
                let reserved = state.totals.get(&resource_key).copied().unwrap_or_default();
                return ResourceReservationDecision::Reserved {
                    effective_available: available.saturating_sub(reserved),
                    reserved,
                };
            }
            Self::release_locked(&mut state, &trade_no);
        }

        let reserved = state.totals.get(&resource_key).copied().unwrap_or_default();
        // Restored reservations are represented in both stores. Taking the
        // component-wise maximum preserves cross-process DB facts without
        // charging restored in-memory reservations twice.
        let effective_available = available.saturating_sub(reserved.component_max(persisted));
        let energy_shortfall = required.energy.saturating_sub(effective_available.energy);
        let bandwidth_shortfall = required.bandwidth.saturating_sub(effective_available.bandwidth);

        if energy_shortfall == 0 && bandwidth_shortfall == 0 {
            state.totals.entry(resource_key.clone()).or_default().add_assign(required);
            state.by_trade_no.insert(trade_no, (resource_key, required));
            ResourceReservationDecision::Reserved { effective_available, reserved }
        } else {
            ResourceReservationDecision::NeedMore(ResourceShortfall {
                energy: energy_shortfall,
                bandwidth: bandwidth_shortfall,
                effective_available,
                reserved,
            })
        }
    }

    pub(crate) fn release(&self, trade_no: &str) {
        let mut state = self.inner.lock().expect("resource reservation lock poisoned");
        Self::release_locked(&mut state, trade_no);
    }

    #[cfg(test)]
    fn reserved_for(&self, resource_key: &str) -> ResourceBudget {
        self.inner
            .lock()
            .expect("resource reservation lock poisoned")
            .totals
            .get(resource_key)
            .copied()
            .unwrap_or_default()
    }

    fn release_locked(state: &mut ResourceReservationState, trade_no: &str) {
        let Some((key, budget)) = state.by_trade_no.remove(trade_no) else {
            return;
        };

        if let Some(total) = state.totals.get_mut(&key) {
            total.sub_assign_saturating(budget);
            if *total == ResourceBudget::default() {
                state.totals.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod withdraw_resource_reservation_tests {
    use super::*;

    #[test]
    fn same_address_pending_orders_reserve_against_shared_available_energy() {
        let manager = WithdrawResourceReservationManager::default();
        let key = "TAy4";
        let available = ResourceBudget::new(20_000, 1_000);
        let required = ResourceBudget::new(13_532, 345);

        assert!(matches!(
            manager.reserve_if_available(key, "A", required, available),
            ResourceReservationDecision::Reserved { .. }
        ));

        let decision = manager.reserve_if_available(key, "B", required, available);
        assert_eq!(
            decision,
            ResourceReservationDecision::NeedMore(ResourceShortfall {
                energy: 7_064,
                bandwidth: 0,
                effective_available: ResourceBudget::new(6_468, 655),
                reserved: required,
            })
        );
        assert_eq!(manager.reserved_for(&key), required);
    }

    #[test]
    fn insufficient_resource_does_not_reserve_memory_budget() {
        let manager = WithdrawResourceReservationManager::default();
        let key = "TAy4";
        let available = ResourceBudget::new(1_000, 1_000);
        let required = ResourceBudget::new(13_532, 345);

        assert!(matches!(
            manager.reserve_if_available(key, "A", required, available),
            ResourceReservationDecision::NeedMore(_)
        ));

        assert_eq!(manager.reserved_for(key), ResourceBudget::default());
    }

    #[test]
    fn different_addresses_reserve_independently() {
        let manager = WithdrawResourceReservationManager::default();
        let available = ResourceBudget::new(20_000, 1_000);
        let required = ResourceBudget::new(13_532, 345);

        assert!(matches!(
            manager.reserve_if_available("TAy4", "A", required, available),
            ResourceReservationDecision::Reserved { .. }
        ));
        assert!(matches!(
            manager.reserve_if_available("TBxx", "B", required, available),
            ResourceReservationDecision::Reserved { .. }
        ));
    }

    #[test]
    fn release_removes_pending_budget() {
        let manager = WithdrawResourceReservationManager::default();
        let key = "TAy4";
        let available = ResourceBudget::new(20_000, 1_000);
        let required = ResourceBudget::new(13_532, 345);

        manager.reserve_if_available(key, "A", required, available);
        manager.release("A");

        assert_eq!(manager.reserved_for(&key), ResourceBudget::default());
        assert!(matches!(
            manager.reserve_if_available(key, "B", required, available),
            ResourceReservationDecision::Reserved { .. }
        ));
    }

    #[test]
    fn restore_rebuilds_each_order_once() {
        let manager = WithdrawResourceReservationManager::default();
        let required = ResourceBudget::new(13_532, 345);

        assert!(manager.restore("TAy4", "A", required));
        assert!(!manager.restore("TAy4", "A", required));

        assert_eq!(manager.reserved_for("TAy4"), required);
        assert!(matches!(
            manager.reserve_if_available("TAy4", "B", required, ResourceBudget::new(20_000, 1_000)),
            ResourceReservationDecision::NeedMore(_)
        ));
    }

    #[test]
    fn persisted_and_restored_budget_is_not_subtracted_twice() {
        let manager = WithdrawResourceReservationManager::default();
        let required = ResourceBudget::new(13_532, 0);
        let available = ResourceBudget::new(27_112, 0);

        assert!(manager.restore("TAy4", "A", required));

        assert!(matches!(
            manager.reserve_with_persisted_budget("TAy4", "B", required, available, required),
            ResourceReservationDecision::Reserved {
                effective_available: ResourceBudget { energy: 13_580, bandwidth: 0 },
                reserved: ResourceBudget { energy: 13_532, bandwidth: 0 },
            }
        ));
    }
}
