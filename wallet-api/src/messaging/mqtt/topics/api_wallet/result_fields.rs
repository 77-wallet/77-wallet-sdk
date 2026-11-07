#[derive(Debug, Default, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwmResultTxFee {
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_empty_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) native_fee: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) bandwidth: Option<u64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) energy: Option<u64>,
}

pub(crate) fn deserialize_optional_non_empty_string<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = <Option<serde_json::Value> as serde::Deserialize>::deserialize(deserializer)?;
    Ok(value.and_then(value_to_non_empty_string))
}

fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = <Option<serde_json::Value> as serde::Deserialize>::deserialize(deserializer)?;
    Ok(value.and_then(value_to_u64))
}

fn value_to_non_empty_string(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(item) => {
            let item = item.trim();
            if item.is_empty() { None } else { Some(item.to_string()) }
        }
        serde_json::Value::Number(item) => Some(item.to_string()),
        _ => None,
    }
}

fn value_to_u64(value: serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(item) => item
            .as_u64()
            .or_else(|| item.as_i64().and_then(|item| u64::try_from(item).ok()))
            .or_else(|| {
                item.as_f64().and_then(|item| {
                    if item.is_finite() && item >= 0.0 && item.fract() == 0.0 {
                        Some(item as u64)
                    } else {
                        None
                    }
                })
            }),
        serde_json::Value::String(item) => item.trim().parse::<u64>().ok(),
        _ => None,
    }
}
