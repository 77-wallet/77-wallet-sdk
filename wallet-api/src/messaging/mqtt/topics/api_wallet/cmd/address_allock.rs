use wallet_database::repositories::api_wallet::{
    expand_batch::ExpandBatchRepo, wallet::ApiWalletRepo,
};
use wallet_transport_backend::request::api_wallet::{
    address::ExpandAddressCompleteReq, msg::MsgAckReq,
};

use crate::{context::Context, domain::api_wallet::account::ApiAccountDomain};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

pub(crate) static EXPAND_INDEX_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// biz_type = AWM_CMD_ADDR_EXPAND
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AwmCmdAddrExpandMsg {
    /// 扩容类型： CHA_ALL / CHA_INDEX
    #[serde(rename = "type")]
    pub typ: AddressAllockType,
    #[serde(rename = "chain")]
    pub chain_code: String,
    pub index: Option<i32>,
    pub uid: String,
    /// 扩容编号  
    pub serial_no: String,
    /// 扩容数量（可空，CHA_BATCH 类型时有效）
    #[serde(
        deserialize_with = "wallet_utils::serde_func::string_to_u32",
        serialize_with = "wallet_utils::serde_func::u32_to_string"
    )]
    pub number: u32,
    /// 批次编号
    pub batch_id: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressAllockType {
    ChaBatch,
    ChaIndex,
}

// 地址池扩容
impl AwmCmdAddrExpandMsg {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
        msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 确认消息
        tracing::debug!(msg_id=%msg_id, "确认收到消息");
        let backend = ctx.get_global_backend_api();
        let mut msg_ack_req = MsgAckReq::default();
        msg_ack_req.push(msg_id);
        backend.msg_ack(msg_ack_req).await?;

        let pool = ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_uid(&pool, &self.uid).await?;
        if api_wallet.is_none() {
            tracing::warn!(uid=%self.uid, "钱包不存在, 不执行扩容");
            backend
                .expand_address_complete(ExpandAddressCompleteReq::new(
                    &self.uid,
                    &self.batch_id,
                    &self.serial_no,
                    false,
                    Some("api wallet not found"),
                ))
                .await?;
            return Ok(());
        }

        ExpandBatchRepo::create_batch(
            &pool,
            &self.uid,
            &self.batch_id,
            &self.serial_no,
            &self.chain_code,
            self.number as i32,
        )
        .await?;

        tracing::info!(uid=%self.uid, chain_code=%self.chain_code, number=%self.number, index=?self.index, batch_id=%self.batch_id, msg_id=%msg_id, "开始处理地址扩容请求");
        // 提交扩容任务
        tracing::info!(msg_id=%msg_id, uid=%self.uid, chain_code=%self.chain_code, "提交扩容任务给Actor管理器");
        // ✅ 事实已形成：batch 已入库
        if let Some(tx) = ctx.get_expand_event_tx().await {
            tx.send(crate::infrastructure::expand_address::event::ExpandEvent::HintScan).await.ok();
        }

        Ok(())
    }

    pub(crate) async fn get_needed_indices(
        ctx: &'static Context,
        typ: &AddressAllockType,
        uid: &str,
        chain_code: &str,
        batch_id: &str,
        number: u32,
        index: Option<i32>,
        current_task_id: Option<&str>,
    ) -> Result<Vec<i32>, crate::error::service::ServiceError> {
        tracing::debug!(uid=%uid, chain_code=%chain_code, number=%number, index=?index, current_task_id=?current_task_id, "开始计算需要扩容的索引");

        // 使用锁来防止并发问题
        tracing::info!(uid=%uid, chain_code=%chain_code, "尝试获取扩容索引计算锁");
        let _guard = EXPAND_INDEX_LOCK.lock().await;
        tracing::info!(uid=%uid, chain_code=%chain_code, "已获取扩容索引计算锁");

        let needed_indices = match typ {
            AddressAllockType::ChaBatch => {
                tracing::debug!(uid=%uid, chain_code=%chain_code, "处理批量扩容类型");
                let next = ApiAccountDomain::calculate_indices_for_expansion_with_ctx(
                    ctx, uid, chain_code, batch_id, number,
                )
                .await?;
                next
            }
            AddressAllockType::ChaIndex => {
                tracing::debug!(uid=%uid, chain_code=%chain_code, "处理单索引扩容类型");
                if let Some(index) = index {
                    tracing::info!(uid=%uid, chain_code=%chain_code, target_index=%index, "指定了单索引进行扩容");
                    vec![index]
                } else {
                    tracing::warn!(uid=%uid, chain_code=%chain_code, "单索引扩容但未指定索引值");
                    vec![]
                }
            }
        };

        tracing::info!(uid=%uid, chain_code=%chain_code, result_count=%needed_indices.len(), result_indices=?needed_indices, "索引计算完成");
        Ok(needed_indices)
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {

    use crate::{infrastructure::task_queue::mqtt_api::ApiMqttStruct, messaging::mqtt::Message};

    #[test]
    fn deserialize() {
        // 69115152444c0b49fc7b9f3c	AwmCmdAddrExpand	{"data":{"batchId":"fefsdfdsfdsfdsf","chain":"tron","index":null,"number":"3","serialNo":"tron_88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640","type":"CHA_BATCH","uid":"703dc9ffe712d3ced169cee62c3c9c8118ce822bd00d49650e02df80ba0fcc30"},"eventNo":"1987712693663371264","eventType":"3","secret":"jnRkLB2TnTDOLsfqsOGsFlnMyoL4qJcKNeNuaFejctA=","sign":"rajb0qK3NJNnwfhgYvGiT1jw1nL8cREURz4M+d3QZW8fhJRVNb2YknT8qLu2jbfw3FqIrV27Nc6t7dPqz6IqDg==","time":1762742610}	2	111	3	2025-11-10T02:43:31Z	2025-11-13T05:47:42Z	Business error: api wallet error: Api Account error: Expand address not done yet
        let data = "{\"bizType\":\"AWM_CMD_ADDR_EXPAND\",\"body\":{\"data\":{\"chain\":\"tron\",\"index\":null,\"number\":\"50\",\"serialNo\":\"tron_88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640\",\"type\":\"CHA_BATCH\",\"uid\":\"88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640\"},\"eventNo\":\"1987712693663371264\",\"eventType\":\"3\",\"secret\":\"jnRkLB2TnTDOLsfqsOGsFlnMyoL4qJcKNeNuaFejctA=\",\"sign\":\"rajb0qK3NJNnwfhgYvGiT1jw1nL8cREURz4M+d3QZW8fhJRVNb2YknT8qLu2jbfw3FqIrV27Nc6t7dPqz6IqDg==\",\"time\":1762742610},\"clientId\":\"df1b2982f3240f55fa8769e38e747010\",\"deviceType\":\"ANDROID\",\"sn\":\"5a748300e76e023cea05523c103763a7976bdfb085c24f9713646ae2faa5949d\",\"msgId\":\"68d4fdcdab00e34b73ef17a0\"}";

        let msg: Message = serde_json::from_str(data).unwrap();
        assert_eq!(format!("{}", msg.biz_type), "AwmCmdAddrExpand");

        let msg: ApiMqttStruct = serde_json::from_value(msg.body).unwrap();
        assert_eq!(format!("{:?}", msg.event_type), "AwmCmdAddrExpand");
    }

    fn sort_vec<T: Ord>(items: &mut Vec<T>) {
        items.sort();
    }

    #[test]
    fn test_sort_vec() {
        let mut items = vec![
            83, 80, 92, 101, 82, 90, 107, 102, 55, 50, 54, 56, 88, 87, 78, 74, 108, 100, 71, 61,
            62, 105, 60, 91, 104, 73, 67, 66, 64, 69, 70, 53, 103, 86, 77, 106, 76, 51, 94, 109,
            79, 93, 52, 57, 96, 72, 63, 65, 95, 68,
        ];

        sort_vec(&mut items);
        let len = items.len();
        assert_eq!(len, 50);

        let mut items_2 = vec![
            65, 83, 74, 106, 82, 94, 72, 55, 69, 86, 108, 57, 105, 103, 70, 67, 87, 88, 54, 64, 95,
            80, 109, 66, 61, 102, 63, 101, 91, 100, 96, 76, 62, 77, 90, 53, 104, 107, 92, 73, 56,
            60, 68, 78, 79, 93, 71,
        ];
        sort_vec(&mut items_2);
        let len = items_2.len();
        assert_eq!(len, 47);
    }
}
