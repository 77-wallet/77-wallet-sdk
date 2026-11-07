//! Low-level setup local to ordinary balance regression scenarios.
use crate::harness::ordinary_business::{OWNER, Scenario};
use serde_json::json;
use wallet_database::{
    entities::bill::{BillKind, BillStatus},
    repositories::bill::BillRepo,
};

pub(super) async fn given_hidden_inactive_identity(s: &Scenario) -> anyhow::Result<()> {
    sqlx::query("UPDATE assets SET status=0").execute(s.pool.write_ref()).await?;
    sqlx::query("UPDATE chain SET status=0").execute(s.pool.write_ref()).await?;
    sqlx::query("UPDATE account SET status=0").execute(s.pool.write_ref()).await?;
    Ok(())
}
pub(super) async fn then_asset_is_hidden(s: &Scenario) -> anyhow::Result<()> {
    let status: i64 =
        sqlx::query_scalar("SELECT status FROM assets").fetch_one(s.pool.read_ref()).await?;
    assert_eq!(status, 0);
    Ok(())
}
pub(super) async fn given_successful_bill_response(
    s: &Scenario,
) -> anyhow::Result<(String, String)> {
    let hash = format!("0x{}", "11".repeat(32));
    let bill = BillRepo::build_bill(
        hash.clone(),
        OWNER.into(),
        crate::harness::ordinary_business::OTHER.into(),
        1.0,
        "eth".into(),
        "ETH".into(),
        false,
        BillKind::Transfer,
        String::new(),
    );
    BillRepo::create(bill, &s.pool).await?;
    let bill = BillRepo::get_by_hash_and_owner(&hash, OWNER, &s.pool).await?;
    let receipt = json!({
        "transactionHash":hash,"transactionIndex":"0x0", "blockNumber":"0x1",
        "blockHash":format!("0x{}", "22".repeat(32)),
        "from":OWNER,"to":crate::harness::ordinary_business::OTHER,
        "status":"0x1","type":"0x0","gasUsed":"0x5208",
        "effectiveGasPrice":"0x1","cumulativeGasUsed":"0x5208",
        "logs":[],"logsBloom":format!("0x{}", "00".repeat(256))
    });
    let empty_block: alloy::rpc::types::Block = Default::default();
    let mut block = serde_json::to_value(empty_block)?;
    block["number"] = json!("0x1");
    block["timestamp"] = json!("0x1");
    s.rpc.reply("eth_getTransactionReceipt", receipt);
    s.rpc.reply("eth_getBlockByHash", block);
    Ok((hash, bill.id.to_string()))
}
pub(super) async fn then_bill_is_successful(s: &Scenario, hash: &str) -> anyhow::Result<()> {
    let stored = BillRepo::get_by_hash_and_owner(hash, OWNER, &s.pool).await?;
    assert_eq!(stored.status, BillStatus::Success.to_i8());
    Ok(())
}
