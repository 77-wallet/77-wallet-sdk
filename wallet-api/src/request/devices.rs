use wallet_database::entities::device::CreateDeviceEntity;
use wallet_transport_backend::request::DeviceInitReq;

#[derive(Debug)]
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
