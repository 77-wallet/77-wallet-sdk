use wallet_transport::client::HttpClient;

use crate::{consts::BASE_URL, response::BackendResponse};

pub async fn send_request(
    method: &str,
    base_url: Option<String>,
    path: &str,
    args: Option<std::collections::HashMap<String, String>>,
    headers: Option<std::collections::HashMap<String, String>>,
) -> Result<std::collections::HashMap<String, serde_json::Value>, crate::Error> {
    let method = wallet_utils::parse_func::method_from_str(method)?;

    let base_url = base_url.unwrap_or(BASE_URL.to_string());
    let url = format!("{base_url}/{path}");
    let client = reqwest::Client::new();

    // 如果方法是 GET，则将参数添加到 URL
    let mut request = if method == reqwest::Method::GET {
        let args = args
            .unwrap_or_default()
            .into_iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect::<Vec<String>>()
            .join("&");
        let url_with_params = if !args.is_empty() {
            format!("{}?{}", url, args)
        } else {
            url.to_string()
        };
        client.request(method, url_with_params)
    } else {
        // 如果不是 GET 方法，则将参数作为请求体发送
        let body_content = wallet_utils::serde_func::serde_to_string(&args)?;
        client.request(method, url).body(body_content)
    };

    for (key, value) in headers.unwrap_or_default() {
        request = request.header(key, value);
    }

    let response = request
        .send()
        .await
        .map_err(|e| wallet_utils::Error::Http(e.into()))?;

    if !response.status().is_success() {
        return Err(
            wallet_utils::Error::Http(wallet_utils::HttpError::NonSuccessStatus(response.status()))
                .into(),
        );
    }
    let res = response
        .json::<String>()
        .await
        .map_err(|e| wallet_utils::Error::Http(e.into()))?;
    let res: BackendResponse = wallet_utils::serde_func::serde_from_str(&res)?;

    res.process()
}

pub async fn _send_request(
    base_url: Option<String>,
    method: &str,
    path: &str,
    args: Option<std::collections::HashMap<String, String>>,
    headers: Option<std::collections::HashMap<String, String>>,
) -> Result<std::collections::HashMap<String, serde_json::Value>, crate::Error> {
    let base_url = base_url.unwrap_or(BASE_URL.to_string());
    let client = HttpClient::new(&base_url, headers, None)?;

    let method = wallet_utils::parse_func::method_from_str(method)?;

    let res = match method {
        reqwest::Method::POST => {
            let mut builder = client.post(path);
            if let Some(args) = args {
                builder = builder.json(args);
            }
            builder.send::<String>().await?
        }
        reqwest::Method::GET => client.get(path).query(args).send::<String>().await?,
        _ => client.get(path).json(args).send::<String>().await?,
    };
    let res: BackendResponse = wallet_utils::serde_func::serde_from_str(&res)?;

    res.process()
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use crate::http::_send_request;

    #[tokio::test]
    async fn test_send_request() -> Result<(), Box<dyn std::error::Error>> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let data = r#"{
        "deviceType": "ANDROID",
        "sn": "2",
        "code": "2",
        "systemVer": "3",
        "iemi": "4",
        "meid": "5",
        "iccid": "6",
        "mem": "7"
    }"#;

        let json: std::collections::HashMap<String, String> = serde_json::from_str(&data)?;
        let base_url = "http://api.wallet.net".to_string();
        let method = "POST";
        let path = "device/init";
        let res = _send_request(Some(base_url), method, path, Some(json), Some(headers))
            .await
            .unwrap();

        println!("{:?}", res);

        Ok(())
    }

    #[tokio::test]
    async fn test() -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder().build()?;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse()?);

        let data = r#"{
        "deviceType": "ANDROID",
        "sn": "2",
        "code": "2",
        "systemVer": "3",
        "iemi": "4",
        "meid": "5",
        "iccid": "6",
        "mem": "7"
    }"#;

        let json: serde_json::Value = serde_json::from_str(&data)?;

        let request = client
            .request(reqwest::Method::POST, "http://api.wallet.net/device/init")
            .headers(headers)
            .json(&json);

        let response = request.send().await?;
        let body = response.text().await?;

        println!("{}", body);

        Ok(())
    }
}
