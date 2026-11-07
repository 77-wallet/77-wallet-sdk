use wallet_database::entities::device::CreateDeviceEntity;
use wallet_transport_backend::request::DeviceInitReq;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct InitDeviceReq {
    pub device_type: String,
    pub sn: String,
    pub code: String,
    pub system_ver: String,
    pub iemi: Option<String>,
    pub meid: Option<String>,
    pub iccid: Option<String>,
    pub mem: Option<String>,
    pub app_id: Option<String>,
    pub package_id: Option<String>,
    pub app_version: String,
    // // 是否通过邀请进入的。
    // pub invitee: bool,
}

impl Default for InitDeviceReq {
    fn default() -> Self {
        Self {
            device_type: "ios".to_string(),
            sn: "xxx12313000899".to_string(),
            code: "aaaccc".to_string(),
            system_ver: "cc".to_string(),
            iemi: None,
            meid: None,
            iccid: None,
            mem: None,
            app_id: None,
            package_id: None,
            app_version: "1.2.3".to_string(),
            // invitee: false,
        }
    }
}

impl From<&InitDeviceReq> for DeviceInitReq {
    fn from(value: &InitDeviceReq) -> Self {
        Self {
            device_type: value.device_type.to_string(),
            sn: value.sn.to_string(),
            code: value.code.to_string(),
            system_ver: value.system_ver.to_string(),
            iemi: value.iemi.to_owned(),
            meid: value.meid.to_owned(),
            iccid: value.iccid.to_owned(),
            mem: value.mem.to_owned(),
            // invitee: value.invitee,
        }
    }
}

impl From<&InitDeviceReq> for CreateDeviceEntity {
    fn from(value: &InitDeviceReq) -> Self {
        Self {
            device_type: value.device_type.to_string(),
            sn: value.sn.to_string(),
            code: value.code.to_string(),
            system_ver: value.system_ver.to_string(),
            iemi: value.iemi.to_owned(),
            meid: value.meid.to_owned(),
            iccid: value.iccid.to_owned(),
            mem: value.mem.to_owned(),
            app_id: value.app_id.to_owned(),
            is_init: 0,
            language_init: 0,
        }
    }
}
