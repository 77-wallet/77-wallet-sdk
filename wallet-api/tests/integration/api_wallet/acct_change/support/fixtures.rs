use serde_json::{Value, json};

pub(crate) struct AcctChangeFixture {
    pub(crate) msg_id: String,
    pub(crate) chain_code: String,
    pub(crate) token: String,
    pub(crate) address: String,
    pub(crate) payload: Value,
}

impl AcctChangeFixture {
    pub(crate) fn api_wallet_sol_usdc_symbol_mismatch() -> Self {
        let token = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        let address = "3jVrVbEPDd35piQUxur1Gki8bkz4XkhZTXZHmfSnmHEd";
        let now = wallet_utils::time::now();
        let msg_id = format!("bug-sol-usdc-{}", now.timestamp_millis());
        let payload = json!({
            "appId": "100d855909c0d553cf9",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 405834872,
                "chainCode": "sol",
                "fromAddr": "9PG6RaXiNm1x5jcVHosc1LnwUgcE3NLLLd7yLHfqStYM",
                "isMultisig": 0,
                "status": true,
                "symbol": "usd coin",
                "toAddr": address,
                "token": token,
                "transactionFee": 0.002045217,
                "transactionTime": "2026-03-12 03:46:40",
                "transferType": 0,
                "txHash": "56jdRtHj6LWHLiSz86tKLG5dj8RXbLHNKW8VVmQqEfYbpmdkb1w5Ko8JpdyH2iSy9J56175Yie3vJboGgyXfrryh",
                "txKind": 1,
                "value": 1.1,
                "valueUsdt": 1.09994888401493623
            },
            "clientId": "4206b0fecd683a1505d24a135b606e9c",
            "deviceType": "ANDROID",
            "sn": "b35f7b556b87c87bb1928ea6ab12ef6918b71f5c37fbd53b88e9353ea2093f0b",
            "uid": "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e",
            "walletType": "API_WAW",
            "msgId": msg_id
        });

        Self::new(msg_id, "sol", token, address, payload)
    }

    pub(crate) fn normal_eth_usdt_symbol_mismatch() -> Self {
        let token = "0xdac17f958d2ee523a2206206994597c13d831ec7";
        let address = "0x148805B49819371EEF9A822f7F880b42Cf67834D";
        let now = wallet_utils::time::now();
        let msg_id = format!("normal-bug-usdt-{}", now.timestamp_millis());
        let payload = json!({
            "msgId": msg_id,
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 21342785,
                "chainCode": "eth",
                "fromAddr": "0x8F1E2a99CB688587c02B8b836Ba9Ca39dC60D63B",
                "isMultisig": 0,
                "notes": "acct-change-symbol-mismatch",
                "queueId": "",
                "status": true,
                "symbol": "tether usd",
                "toAddr": address,
                "token": token,
                "transactionFee": 0.00135940441821096,
                "transactionTime": "2026-03-12 10:13:47",
                "transferType": 0,
                "txHash": "0xaaa362dfd318f4da95e2d1e71c8c2a2ceabc8fd5df85e7c144843e6fc55f25e0",
                "txKind": 1,
                "value": 0.1112
            },
            "clientId": "7552bd49a9407eb98164c129d11da7e2",
            "deviceType": "IOS",
            "sn": "5bb0eada7cb7290b5d196362e6def48dcb9703e1468c0fb28eb7dd61073875e6",
            "walletType": "NORMAL_WALLET"
        });

        Self::new(msg_id, "eth", token, address, payload)
    }

    pub(crate) fn normal_eth_native_missing_token() -> Self {
        let address = "0x6F17DfC6a4E6B1f7A0A0eD3a4b2f1Bf49E2d0B73";
        let now = wallet_utils::time::now();
        let msg_id = format!("normal-bug-native-{}", now.timestamp_millis());
        let payload = json!({
            "msgId": msg_id,
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 21342786,
                "chainCode": "eth",
                "fromAddr": "0x1111111111111111111111111111111111111111",
                "isMultisig": 0,
                "notes": "acct-change-native-token-missing",
                "queueId": "",
                "status": true,
                "symbol": "ether",
                "toAddr": address,
                "transactionFee": 0.00135940441821096,
                "transactionTime": "2026-03-12 10:13:48",
                "transferType": 0,
                "txHash": "0xbbb362dfd318f4da95e2d1e71c8c2a2ceabc8fd5df85e7c144843e6fc55f25e1",
                "txKind": 1,
                "value": 0.2223
            },
            "clientId": "7552bd49a9407eb98164c129d11da7e3",
            "deviceType": "IOS",
            "sn": "5bb0eada7cb7290b5d196362e6def48dcb9703e1468c0fb28eb7dd61073875e7",
            "walletType": "NORMAL_WALLET"
        });

        Self::new(msg_id, "eth", "", address, payload)
    }

    fn new(msg_id: String, chain_code: &str, token: &str, address: &str, payload: Value) -> Self {
        Self {
            msg_id,
            chain_code: chain_code.to_string(),
            token: token.to_string(),
            address: address.to_string(),
            payload,
        }
    }
}
