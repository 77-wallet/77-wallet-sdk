use crate::context::Context;
use wallet_database::entities::announcement::CreateAnnouncementVo;

use crate::{
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
    service::announcement::AnnouncementService,
};

/*
{
    "clientId": "666",
    "sn": "666",
    "deviceType": "ANDROID",
    "bizType": "TOKEN_PRICE_CHANGE",
    "body": {
        "chainCode": "polygon",
        "code": "chain",
        "defaultToken": false,
        "enable": true,
        "marketValue": 6644971.07,
        "master": false,
        "name": "Chain Games",
        "price": 0.021205427084188898,
        "status": false,
        "tokenAddress": "0xd55fce7cdab84d84f2ef3f99816d765a2a94a509",
        "unit": 18,
    }
}
*/
// biz_type = TOKEN_PRICE_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BulletinMsg {
    // pub body: Announcementbody,
    /// 公告id
    pub id: String,
    /// 公告标题
    pub title: String,
    /// 公告内容
    pub content: String,
    /// 语言
    pub language: String,
    pub operation: Option<Operation>,
    pub send_time: Option<String>,
    pub status: Option<String>,
    // #[serde(skip_serializing)]
    // pub update_time: Option<String>,
    // #[serde(skip_serializing)]
    // pub create_time: Option<String>,
    // #[serde(skip_serializing)]
    // pub r#type: Option<String>,
}

// "body": {
//     "content": "fdsk;ewrjkopwr923rljflksdjfjisd",
//     "createTime": "2024-08-21 16:09:26.468",
//     "id": "66ebca44cfcc03419d0c4c4a",
//     "language": "CHINESE_SIMPLIFIED",
//     "operator": "xiaohai",
//     "sendTime": "2024-08-21 16:02:26.468",
//     "status": "sending",
//     "title": "公告测试标题5",
//     "type": "2",
//     "updateTime": "2024-09-19 15:50:10.011"
// }

// #[derive(Debug, serde::Deserialize, serde::Serialize)]
// #[serde(rename_all = "camelCase")]
// pub struct Announcementbody {
//     /// 公告id
//     pub id: String,
//     /// 公告标题
//     pub title: String,
//     /// 公告内容
//     pub content: String,
//     /// 语言
//     pub language: String,
//     #[serde(skip_serializing)]
//     pub operator: String,
//     #[serde(skip_serializing)]
//     pub send_time: String,
//     #[serde(skip_serializing)]
//     pub status: String,
//     #[serde(skip_serializing)]
//     pub update_time: String,
//     #[serde(skip_serializing)]
//     pub create_time: String,
//     #[serde(skip_serializing)]
//     pub r#type: String,
// }

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Send,
    Delete,
}

impl BulletinMsg {
    pub(crate) async fn exec(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.exec_with_ctx(_msg_id, ctx).await
    }

    pub(crate) async fn exec_with_ctx(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let Self { id, operation, .. } = self;
        if let Some(operation) = operation {
            match operation {
                Operation::Send => {
                    let send_time =
                        self.send_time.clone().or(Some(wallet_utils::time::now().to_string()));
                    let input = CreateAnnouncementVo {
                        id: self.id.clone(),
                        title: self.title.clone(),
                        content: self.content.clone(),
                        language: self.language.clone(),
                        status: 0,
                        send_time,
                    };
                    AnnouncementService::new(ctx).add(vec![input]).await?;
                }
                Operation::Delete => {
                    AnnouncementService::new(ctx).physical_delete(id).await?;
                }
            }
        }
        let data = NotifyEvent::BulletinMsg(self.to_owned());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        Ok(())
    }
}
