//! Persisted nonce ownership and deterministic unsent failure reuse.

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NonceClaim {
    pub kind: String,
    pub nonce: i64,
    pub raw_tx: Option<String>,
    pub tx_hash: Option<String>,
    pub err_code: Option<i64>,
    pub err_msg: Option<String>,
    pub failure_stage: Option<i64>,
    pub last_broadcast_at: Option<String>,
    pub uncertain: bool,
    pub executed: bool,
    pub terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithdrawNonceDecision {
    Ready(i64),
    Wait,
    TerminalGap(i64),
}

pub const COLLECT_NONCE_GAP_REPORTED: &str = "nonce_gap_reported:";

impl NonceClaim {
    fn is_reported_collect_gap(&self) -> bool {
        self.kind == "collect"
            && self.terminal
            && !self.executed
            && self.err_code.is_some()
            && self.err_msg.as_deref().is_some_and(|m| m.starts_with(COLLECT_NONCE_GAP_REPORTED))
    }

    pub fn is_reusable_failure(&self) -> bool {
        // Only known pre-confirmation failure stages can prove an unsent failure.
        // Missing/unknown/chain/ACK stages never release an existing signature;
        // send, uncertainty and execution facts override even a known stage.
        self.kind == "withdraw"
            && self.err_code.is_some_and(|code| code > 0)
            && self.terminal
            && self.failure_stage.is_some_and(|stage| {
                use crate::entities::api_withdraw::WithdrawFailureStage;
                stage == WithdrawFailureStage::Build as i64
                    || stage == WithdrawFailureStage::Broadcast as i64
            })
            && self.last_broadcast_at.is_none()
            && !self.uncertain
            && !self.executed
    }
}

pub fn withdraw_nonce_decision(
    chain_next: i64,
    local_last: i64,
    claims: &[NonceClaim],
) -> WithdrawNonceDecision {
    use WithdrawNonceDecision::*;
    if chain_next < 0 {
        return Wait;
    }
    if local_last < chain_next {
        return Ready(chain_next);
    }
    let mut slots: std::collections::BTreeMap<i64, bool> = std::collections::BTreeMap::new();
    for claim in claims.iter().filter(|c| c.nonce >= chain_next && c.nonce <= local_last) {
        if !claim.is_reusable_failure()
            && (claim.terminal || claim.executed)
            && !(claim.is_reported_collect_gap() && claim.nonce > chain_next)
        {
            return Wait;
        }
        *slots.entry(claim.nonce).or_insert(true) &= claim.is_reusable_failure();
    }
    let Some(required) = local_last.checked_sub(chain_next).and_then(|n| n.checked_add(1)) else {
        return Wait;
    };
    if slots.len() as i64 != required {
        // Only a hole before a durable signed owner is evidenced here. A legacy
        // high-water counter can be above all signed facts; never allocate its
        // unknown trailing slots or rewind it. With no signed owner, stay blocked.
        let mut next = chain_next;
        for (&nonce, &reusable) in &slots {
            if nonce > next {
                if claims.iter().any(|c| {
                    c.is_reported_collect_gap() && c.nonce >= chain_next && c.nonce <= next
                }) {
                    return Wait;
                }
                return Ready(next);
            }
            if reusable {
                return TerminalGap(nonce);
            }
            next = match next.checked_add(1) {
                Some(n) => n,
                None => return Wait,
            };
        }
        return Wait;
    }
    // A reported signed gap still owns its slot. Only an earlier unsigned hole
    // can be used; never allocate beyond or reuse this failed signed order.
    if claims.iter().any(|c| c.nonce >= chain_next && c.is_reported_collect_gap()) {
        return Wait;
    }
    let highest_live = slots.iter().rev().find_map(|(n, reusable)| (!reusable).then_some(*n));
    let Some(highest_live) = highest_live else {
        return Ready(chain_next);
    };
    if let Some((nonce, _)) = slots.iter().find(|(n, reusable)| **n < highest_live && **reusable) {
        return TerminalGap(*nonce);
    }
    highest_live.checked_add(1).map(Ready).unwrap_or(Wait)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn shared_nonce_recovery_selects_unsigned_five_before_saved_six() {
        let saved = NonceClaim {
            kind: "collect".into(),
            nonce: 6,
            raw_tx: Some("saved-six".into()),
            tx_hash: Some("hash-six".into()),
            err_code: None,
            err_msg: None,
            failure_stage: None,
            last_broadcast_at: Some("sent".into()),
            uncertain: true,
            executed: false,
            terminal: false,
        };
        assert_eq!(withdraw_nonce_decision(5, 6, &[saved]), WithdrawNonceDecision::Ready(5));
    }
}

#[cfg(test)]
mod recovery_guards {
    use super::*;
    fn claim(nonce: i64) -> NonceClaim {
        NonceClaim {
            kind: "fee".into(),
            nonce,
            raw_tx: Some("bytes".into()),
            tx_hash: Some("hash".into()),
            err_code: None,
            err_msg: None,
            failure_stage: None,
            last_broadcast_at: None,
            uncertain: false,
            executed: false,
            terminal: false,
        }
    }
    #[test]
    fn shared_nonce_recovery_preserves_bare_counter_and_unknown_trailing_slots() {
        assert_eq!(withdraw_nonce_decision(5, 6, &[]), WithdrawNonceDecision::Wait);
        // A durable6 bounds the earlier hole; the unknown trailing7 stays protected.
        assert_eq!(withdraw_nonce_decision(5, 7, &[claim(6)]), WithdrawNonceDecision::Ready(5));
        assert_eq!(withdraw_nonce_decision(6, 7, &[claim(6)]), WithdrawNonceDecision::Wait);
    }
    #[test]
    fn shared_nonce_recovery_keeps_signed_five_and_uncertain_six_owned() {
        let mut six = claim(6);
        six.uncertain = true;
        assert_eq!(
            withdraw_nonce_decision(5, 6, &[claim(5), six]),
            WithdrawNonceDecision::Ready(7)
        );
    }
    #[test]
    fn shared_nonce_recovery_does_not_reuse_signed_terminal_slot() {
        let mut six = claim(6);
        six.terminal = true;
        six.err_code = Some(6008);
        assert_eq!(withdraw_nonce_decision(5, 6, &[six]), WithdrawNonceDecision::Wait);
    }
}

#[cfg(test)]
mod reported_gap_tests {
    use super::*;
    #[test]
    fn reported_collect_gap_retains_six_but_allows_unsigned_five() {
        let claim = NonceClaim {
            kind: "collect".into(),
            nonce: 6,
            raw_tx: Some("raw".into()),
            tx_hash: Some("hash".into()),
            err_code: Some(1),
            err_msg: Some("nonce_gap_reported: chain_next=5 order_nonce=6".into()),
            failure_stage: None,
            last_broadcast_at: Some("sent".into()),
            uncertain: true,
            executed: false,
            terminal: true,
        };
        assert_eq!(
            withdraw_nonce_decision(5, 6, &[claim.clone()]),
            WithdrawNonceDecision::Ready(5)
        );
        let mut five = claim.clone();
        five.nonce = 5;
        five.terminal = false;
        five.err_code = None;
        five.err_msg = None;
        let mut eight = five.clone();
        eight.nonce = 8;
        assert_eq!(
            withdraw_nonce_decision(5, 8, &[five, claim.clone(), eight.clone()]),
            WithdrawNonceDecision::Wait
        );
        assert_eq!(
            withdraw_nonce_decision(5, 8, &[claim.clone(), eight]),
            WithdrawNonceDecision::Ready(5)
        );
        assert_eq!(withdraw_nonce_decision(6, 6, &[claim]), WithdrawNonceDecision::Wait);
    }
}

#[cfg(test)]
mod counter_ahead_gap_tests {
    use super::*;
    #[test]
    fn known_signed_six_proves_earlier_gap_despite_counter_153() {
        let mut six = NonceClaim {
            kind: "collect".into(),
            nonce: 6,
            raw_tx: Some("raw".into()),
            tx_hash: Some("hash".into()),
            err_code: None,
            err_msg: None,
            failure_stage: None,
            last_broadcast_at: Some("sent".into()),
            uncertain: false,
            executed: false,
            terminal: false,
        };
        assert_eq!(
            withdraw_nonce_decision(5, 153, &[six.clone()]),
            WithdrawNonceDecision::Ready(5)
        );
        assert_eq!(withdraw_nonce_decision(5, 153, &[]), WithdrawNonceDecision::Wait);
        let mut five = six.clone();
        five.nonce = 5;
        assert_eq!(
            withdraw_nonce_decision(5, 153, &[five, six.clone()]),
            WithdrawNonceDecision::Wait
        );
        six.err_code = Some(6001);
        six.err_msg = Some("nonce_gap_reported: missing_nonce=5".into());
        six.terminal = true;
        assert_eq!(
            withdraw_nonce_decision(5, 153, &[six.clone()]),
            WithdrawNonceDecision::Ready(5)
        );
        assert_eq!(withdraw_nonce_decision(6, 153, &[six]), WithdrawNonceDecision::Wait);
    }
}
