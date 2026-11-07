use super::transfer_gate::TransferGates;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn transfer_gate_evm_aliases_wait_and_resume_after_release() {
    let gates = TransferGates::default();
    let owner = gates.acquire(" 0xAbC ", "eth").await;
    let waiter = gates.acquire("0xabc", "eth");
    tokio::pin!(waiter);
    assert!(tokio::time::timeout(Duration::from_millis(50), &mut waiter).await.is_err());
    drop(owner);
    assert!(tokio::time::timeout(Duration::from_secs(1), &mut waiter).await.is_ok());
}

#[tokio::test]
async fn transfer_gate_independent_addresses_and_chains_do_not_block() {
    let gates = TransferGates::default();
    let _owner = gates.acquire("sender", "eth").await;
    for (address, chain) in [("other", "eth"), ("sender", "scroll")] {
        assert!(
            tokio::time::timeout(Duration::from_secs(1), gates.acquire(address, chain))
                .await
                .is_ok()
        );
    }
    let _case_sensitive = gates.acquire("AbC", "sol").await;
    assert!(
        tokio::time::timeout(Duration::from_secs(1), gates.acquire("abc", "sol")).await.is_ok()
    );
}

#[tokio::test]
async fn transfer_gate_cancelled_waiter_does_not_poison_slot() {
    let gates = TransferGates::default();
    let owner = gates.acquire("sender", "eth").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), gates.acquire("sender", "eth"))
            .await
            .is_err()
    );
    drop(owner);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), gates.acquire("sender", "eth")).await.is_ok()
    );
}

#[tokio::test]
async fn transfer_gate_aborted_owner_releases_slot() {
    let gates = Arc::new(TransferGates::default());
    let task_gates = gates.clone();
    let (ready, acquired) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(async move {
        let _permit = task_gates.acquire("sender", "eth").await;
        ready.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    acquired.await.unwrap();
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), gates.acquire("sender", "eth")).await.is_ok()
    );
}
