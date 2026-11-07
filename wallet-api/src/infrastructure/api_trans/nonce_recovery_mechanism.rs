use crate::{
    context::Context, error::service::ServiceError,
    infrastructure::nonce::nonce_engine::get_nonce_engine_with_ctx,
};

pub(crate) async fn try_nonce_self_heal(
    ctx: &'static Context,
    trade_no: &str,
    from_addr: &str,
    chain_code: &str,
    error_msg: &str,
    source: &str,
) -> Result<(), ServiceError> {
    let nonce_engine = get_nonce_engine_with_ctx(ctx)?;
    if let Err(err) = nonce_engine.handle_nonce_error(from_addr, chain_code, error_msg).await {
        tracing::warn!(
            trade_no = %trade_no,
            error = %err,
            source = %source,
            "Nonce self-heal failed"
        );
    }
    Ok(())
}
