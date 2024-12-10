use crate::api::ReturnType;

pub mod config;
pub mod env;

pub async fn net_api(
    method: &str,
    base_url: Option<String>,
    path: &str,
    args: Option<std::collections::HashMap<String, String>>,
    headers: Option<std::collections::HashMap<String, String>>,
) -> ReturnType<std::collections::HashMap<String, serde_json::Value>> {
    // wallet_transport_backend::
    wallet_transport_backend::_send_request(base_url, method, path, args, headers)
        .await
        .map_err(crate::ServiceError::TransportBackend)
        .into()
}

#[cfg(test)]
pub(crate) mod tests {

    use crate::net_api;

    #[tokio::test]
    async fn test_net_api() {
        let method = "POST";
        let path = "chain/list";
        let res = net_api(method, None, path, None, None).await;

        println!("[net_api] res: {res:?}");
    }
}
