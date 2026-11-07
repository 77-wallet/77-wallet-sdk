use std::time::Instant;

use crate::init;
use tokio::task::JoinSet;
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::request::api_wallet::swap::ApiInitSwapReq;

#[serial_test::serial]
#[tokio::test]
async fn test_api_wallet_chain_list() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    //     let pub_key = r#"-----BEGIN PUBLIC KEY-----
    // MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEa5VZe6ldctovjscGK1k4Fq/6OMa4C5ke
    // Fb0OW3wf4gYNiPRKBAU47v77BdGjyT6y7tVeuQ714ql4fhTUVWfnMg==
    // -----END PUBLIC KEY-----"#;
    //     GLOBAL_KEY.set_sn("lan48300e76e023cea05523c103763a7976bdfb085c24f9713646ae2faa59524");
    //     GLOBAL_KEY.set_shared_secret(pub_key)?;

    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let total = 150; // 👈 并发请求数，按需要调大
    let start = Instant::now();
    let mut set = JoinSet::new();

    tracing::info!("start concurrent test, total = {}", total);

    for i in 0..total {
        let api = backend_api.clone();
        set.spawn(async move {
            let t0 = Instant::now();
            let res = api.api_wallet_chain_list("2.0.0").await;
            let cost = t0.elapsed().as_millis();

            match res {
                Ok(_) => {
                    tracing::info!(task = i, cost_ms = cost, "api_wallet_chain_list ok");
                    Ok::<_, wallet_transport_backend::Error>(())
                }
                Err(e) => {
                    tracing::error!(
                        task = i,
                        cost_ms = cost,
                        err = %e,
                        "api_wallet_chain_list err"
                    );
                    Err(e)
                }
            }
        });
    }

    let mut ok = 0;
    let mut err = 0;

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(())) => ok += 1,
            Ok(Err(_)) => err += 1,
            Err(e) => {
                err += 1;
                tracing::error!("join err: {}", e);
            }
        }
    }

    tracing::info!(
        ok = ok,
        err = err,
        total = total,
        cost_ms = start.elapsed().as_millis(),
        "concurrent test finished"
    );

    Ok(())
}
