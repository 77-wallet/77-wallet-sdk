use rust_decimal::Decimal;
use serde_json::json;
use wallet_transport_backend::{
    request::api_wallet::wallet::AppConfigReq, response_vo::api_wallet::wallet::AppConfigResp,
};

#[test]
fn app_config_request_uses_backend_camel_case_fields() {
    let request = AppConfigReq { app_id: "app".into(), org_id: "merchant".into() };
    assert_eq!(serde_json::to_value(request).unwrap(), json!({"appId":"app","orgId":"merchant"}));
}

#[test]
fn app_config_decodes_observed_warning_shape_and_preserves_threshold_precision() {
    let response: AppConfigResp = serde_json::from_value(json!({"configs":{
        "balanceNotifyCfgList":[{"amount":"5.000000000000000001","chainCode":"polygon",
        "contractAddress":"","tokenCode":"POL","status":true,"merchantId":"merchant","oskId":"app"}]
    }}))
    .unwrap();
    let item = &response.configs.balance_notify_cfg_list[0];
    assert_eq!(item.amount, "5.000000000000000001".parse::<Decimal>().unwrap());
    assert_eq!(item.chain_code, "polygon");
    assert_eq!(item.contract_address, "");
    assert_eq!(item.token_code, "POL");
    assert!(item.status);
    let number: AppConfigResp = serde_json::from_value(json!({"configs":{
        "balanceNotifyCfgList":[{"amount":5,"chainCode":"tron","contractAddress":"", "tokenCode":"TRX","status":false}]
    }})).unwrap();
    assert_eq!(number.configs.balance_notify_cfg_list[0].amount, Decimal::from(5));
    assert!(!number.configs.balance_notify_cfg_list[0].status);
}

#[test]
fn explicit_empty_config_is_distinct_from_missing_or_malformed_data() {
    let empty: AppConfigResp =
        serde_json::from_value(json!({"configs":{"balanceNotifyCfgList":[]}})).unwrap();
    assert!(empty.configs.balance_notify_cfg_list.is_empty());
    for invalid in [
        json!({}),
        json!({"configs":{}}),
        json!({"configs":null}),
        json!({"configs":{"balanceNotifyCfgList":null}}),
        json!({"configs":{"balanceNotifyCfgList":[{"amount":5}]}}),
    ] {
        assert!(serde_json::from_value::<AppConfigResp>(invalid).is_err());
    }
}
