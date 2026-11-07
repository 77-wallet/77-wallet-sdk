use super::{ChainErrorNormalizer, NonceErrorKind};

#[test]
fn fee_errors_are_not_consumed_nonces() {
    for (message, expected) in [
        ("replacement transaction underpriced", NonceErrorKind::ReplacementUnderpriced),
        ("RPC: Transaction Underpriced", NonceErrorKind::TransactionUnderpriced),
    ] {
        assert_eq!(ChainErrorNormalizer::normalize(message), expected);
    }
}

#[test]
fn known_transaction_means_visibility_only() {
    for message in ["known transaction: 0x123", "already known", "RPC: Already Known"] {
        assert_eq!(ChainErrorNormalizer::normalize(message), NonceErrorKind::AlreadyKnown);
    }
}

#[test]
fn unknown_transaction_is_not_already_known() {
    let message = "RPC: unknown transaction";
    assert_eq!(ChainErrorNormalizer::normalize(message), NonceErrorKind::Other(message.into()));
}

#[test]
fn nonce_consumption_and_future_nonce_remain_distinct() {
    for (message, expected) in [
        ("nonce too low", NonceErrorKind::NonceTooLow),
        ("invalid nonce: too low", NonceErrorKind::NonceTooLow),
        ("nonce too high", NonceErrorKind::NonceTooHigh),
        ("nonce already used", NonceErrorKind::NonceAlreadyUsed),
        ("chain data inconsistent", NonceErrorKind::ChainDataInconsistent),
    ] {
        assert_eq!(ChainErrorNormalizer::normalize(message), expected);
    }
}
