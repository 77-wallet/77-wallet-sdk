//! A debug command must not embed large worker states in each dispatcher branch.
use super::{
    CollectIntent, ShadowCollectCommand, ShadowCollectWorker, ShadowDispatcher, SideEffectWorker,
};
use std::{future::Future, sync::Arc};

#[test]
fn collect_mobile_stack_dispatch_future_is_bounded() {
    fn size<F, Fut>(_: F) -> usize
    where
        F: FnOnce(CollectIntent, Arc<ShadowCollectWorker>, Arc<SideEffectWorker>) -> Fut,
        Fut: Future,
    {
        std::mem::size_of::<Fut>()
    }
    let bytes = size(ShadowDispatcher::execute_intent);
    assert!(bytes <= 16 * 1024, "dispatcher embeds {bytes} bytes of worker states in every poll");
}

#[test]
fn collect_mobile_stack_command_future_is_heap_bounded() {
    fn size<F, Fut>(_: F) -> usize
    where
        F: FnOnce(&'static ShadowCollectWorker, ShadowCollectCommand) -> Fut,
        Fut: Future,
    {
        std::mem::size_of::<Fut>()
    }
    let bytes = size(ShadowCollectWorker::handle);
    assert!(
        bytes <= 4 * 1024,
        "command future is {bytes} bytes; it must not amplify dispatcher stack"
    );
}
