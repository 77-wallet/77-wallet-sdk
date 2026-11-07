#[derive(Debug, thiserror::Error)]
pub enum MultisigQueueError {
    #[error("Multisig Queue already exists")]
    AlreadyExist,
    #[error("Multisig Queue not found")]
    NotFound,
    #[error("the transaction has exec or sign not complete")]
    NotPendingExecStatus,
    #[error("multisig queue is expired")]
    Expired,
    #[error("has already been executed")]
    AlreadyExecuted,
    #[error("cannot cancel")]
    CannotCancel,
    #[error("faile queue,cannot exec or signature")]
    FailedQueue,
}

impl MultisigQueueError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            MultisigQueueError::AlreadyExist => 3700,
            MultisigQueueError::NotFound => 3701,
            MultisigQueueError::NotPendingExecStatus => 3702,
            MultisigQueueError::Expired => 3703,
            MultisigQueueError::AlreadyExecuted => 3704,
            MultisigQueueError::CannotCancel => 3705,
            MultisigQueueError::FailedQueue => 3706,
        }
    }
}
