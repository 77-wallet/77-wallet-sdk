use crate::{
    context::Context,
    messaging::{
        mqtt::topics::api_wallet::result_fields::{
            AwmResultTxFee, deserialize_optional_non_empty_string,
        },
        notify::{FrontendNotifyEvent, event::NotifyEvent},
    },
};
use wallet_database::repositories::api_wallet::{collect::ApiCollectRepo, wallet::ApiWalletRepo};
use wallet_transport_backend::request::api_wallet::msg::MsgAckReq;

// biz_type = AWM_CMD_FEE_RES
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AwmCmdFeeResMsg {
    trade_no: String,
    /// 交易类型： 1 提币 / 2 归集 / 3 归集手续费交易
    #[serde(
        deserialize_with = "wallet_utils::serde_func::string_to_u32",
        serialize_with = "wallet_utils::serde_func::u32_to_string"
    )]
    trade_type: u32,
    /// 交易结果： true 成功 /false 失败
    status: bool,
    /// 失败类型由后端透传给前端展示；FeeRes 业务逻辑当前只关心 status。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fail_type: Option<i32>,
    uid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tx_fee: Option<AwmResultTxFee>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_empty_string",
        skip_serializing_if = "Option::is_none"
    )]
    block_number: Option<String>,
}

// API手续费结果事件
impl AwmCmdFeeResMsg {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        tracing::info!("exec AwmCmdFeeResMsg: {:?}", self);
        self.check_uid(ctx).await?;

        let backend = ctx.get_global_backend_api();
        let mut msg_ack_req = MsgAckReq::default();
        msg_ack_req.push(_msg_id);
        backend.msg_ack(msg_ack_req).await?;

        let data = NotifyEvent::AwmCmdFeeRes(self.to_owned());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }

    pub(crate) async fn check_uid(
        &self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let res = ApiWalletRepo::find_by_uid(&pool, &self.uid).await?;
        if res.is_none() {
            tracing::warn!("AwmCmdFeeResMsg uid not found: {}", self.uid);
            return Ok(());
        }

        // FeeRes 表示外部“手续费问题已解决/手续费已到”的事实
        // Collect 流程的推进依赖 need_service_fee 从 true 变为 false
        if !self.status {
            tracing::warn!("AwmCmdFeeResMsg status is false: {:?}", self);
            return Ok(());
        }

        // 尝试对 api_collect 写入“解除构建阻断”事实
        // - 仅在 need_service_fee = true 时生效（幂等保护）
        // - 若 trade_no 不存在或 need_service_fee 非 true，则 rows_affected = 0
        let funds_pool = ctx.api_transaction_pool()?;
        match ApiCollectRepo::get_api_collect_by_trade_no(&funds_pool, &self.trade_no).await {
            Ok(collect) => {
                let affected =
                    ApiCollectRepo::resolve_need_service_fee(&funds_pool, &self.trade_no).await?;
                if affected == 0 {
                    tracing::warn!(
                        trade_no = %self.trade_no,
                        trade_type = %self.trade_type,
                        need_service_fee = ?collect.need_service_fee,
                        "FeeRes received but resolve_need_service_fee affected 0 rows"
                    );
                } else {
                    tracing::info!(
                        trade_no = %self.trade_no,
                        trade_type = %self.trade_type,
                        affected = %affected,
                        "Resolved need_service_fee due to FeeRes"
                    );
                }

                // 快速触发一次 Shadow 推进（让 TxFeeResAck / Build / Broadcast 尽快发生）
                if let Some(handles) = ctx.get_global_handles().await.upgrade() {
                    if let Some(shadow_system) =
                        handles.get_global_processed_collect_tx_handle().get_shadow_system()
                    {
                        if let Err(e) = shadow_system.trigger_collect(&self.trade_no).await {
                            tracing::warn!(trade_no=%self.trade_no, "Trigger collect shadow failed after FeeRes, but continuing: {:?}", e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    trade_no = %self.trade_no,
                    trade_type = %self.trade_type,
                    error = %e,
                    "FeeRes received but api_collect trade_no not found"
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awm_cmd_fee_res_mock_data_parses_actual_fee_fields() -> anyhow::Result<()> {
        let msg: AwmCmdFeeResMsg = serde_json::from_value(serde_json::json!({
            "tradeNo": "FEE_mock_1",
            "tradeType": "3",
            "status": true,
            "failType": 0,
            "uid": "uid_1",
            "remark": "mock fee result",
            "txFee": {
                "nativeFee": 1.23,
                "bandwidth": "345",
                "energy": 678
            },
            "blockNumber": 12345678
        }))?;

        assert_eq!(msg.trade_no, "FEE_mock_1");
        assert_eq!(msg.trade_type, 3);
        assert!(msg.status);
        assert_eq!(msg.fail_type, Some(0));
        assert_eq!(msg.uid, "uid_1");
        assert_eq!(msg.remark.as_deref(), Some("mock fee result"));
        assert_eq!(msg.block_number.as_deref(), Some("12345678"));

        let tx_fee = msg.tx_fee.as_ref().expect("txFee should parse");
        assert_eq!(tx_fee.native_fee.as_deref(), Some("1.23"));
        assert_eq!(tx_fee.bandwidth, Some(345));
        assert_eq!(tx_fee.energy, Some(678));

        let serialized = serde_json::to_value(&msg)?;
        assert_eq!(serialized["txFee"]["nativeFee"], "1.23");
        assert_eq!(serialized["txFee"]["bandwidth"], 345);
        assert_eq!(serialized["txFee"]["energy"], 678);
        assert_eq!(serialized["blockNumber"], "12345678");
        Ok(())
    }
}
