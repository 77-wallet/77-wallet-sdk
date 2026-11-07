//! Shared receipt-observer lifecycle. A completed round never exhausts retries.
use std::{future::Future, time::Duration};
use tokio::sync::broadcast;

pub(crate) async fn run_receipt_observer<S, F, Fut>(
    mut shutdown: broadcast::Receiver<()>,
    period: Duration,
    name: &str,
    mut state: S,
    mut observe: F,
) where
    F: FnMut(S) -> Fut,
    Fut: Future<Output = S>,
{
    let offset = super::generate_phase_offset(period, name);
    if !super::execute_phase_offset(offset, &mut shutdown).await {
        return;
    }
    let mut interval = super::new_production_interval(period);
    loop {
        state = tokio::select! {
            biased;
            _ = shutdown.recv() => return,
            state = observe(state) => state,
        };
        tokio::select! {
            biased;
            _ = shutdown.recv() => return,
            _ = interval.tick() => {},
        }
    }
}
