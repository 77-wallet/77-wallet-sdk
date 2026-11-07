use crate::batch_transfer::BatchTransferConfig;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum ConsoleCommand {
    ImportConfiguredWallets,
    RefreshWallets,
    RefreshAccountAddresses { wallet_address: String, chain_code: String },
    RefreshBalances { wallet_address: String, account_id: Option<u32>, chain_code: Option<String> },
    SyncWalletAssets { wallet_address: String },
    ImportBind { app_id: String, org_id: String, subaccount_uid: String, withdrawal_uid: String },
    ScanBind { app_id: String, org_id: String, subaccount_uid: String, withdrawal_uid: String },
    FetchPendingWithdrawOrders { withdrawal_uid: String, page_size: i64 },
    ReviewWithdrawOrders { trade_nos: Vec<String>, approve: bool },
    SetChainNode { chain_code: String, rpc_url: String },
    RefreshTaskQueueStatus,
    DiagnoseTrade { trade_no: String },
    LoadTransferTargets { chain_code: String, sub_wallet_address: String },
    RunBatchTransfer { config: BatchTransferConfig },
}

#[cfg(test)]
mod tests {
    use super::ConsoleCommand;

    #[test]
    fn set_chain_node_command_round_trips() {
        let command = ConsoleCommand::SetChainNode {
            chain_code: "tron".to_string(),
            rpc_url: "https://nile.trongrid.io".to_string(),
        };

        let encoded = serde_json::to_string(&command).unwrap();
        let decoded: ConsoleCommand = serde_json::from_str(&encoded).unwrap();

        assert!(matches!(
            decoded,
            ConsoleCommand::SetChainNode { chain_code, rpc_url }
                if chain_code == "tron" && rpc_url == "https://nile.trongrid.io"
        ));
    }

    #[test]
    fn diagnose_trade_command_round_trips() {
        let command = ConsoleCommand::DiagnoseTrade { trade_no: "trade-001".to_string() };

        let encoded = serde_json::to_string(&command).unwrap();
        let decoded: ConsoleCommand = serde_json::from_str(&encoded).unwrap();

        assert!(matches!(
            decoded,
            ConsoleCommand::DiagnoseTrade { trade_no } if trade_no == "trade-001"
        ));
    }

    #[test]
    fn sync_wallet_assets_command_round_trips() {
        let command =
            ConsoleCommand::SyncWalletAssets { wallet_address: "0xwithdrawal".to_string() };

        let encoded = serde_json::to_string(&command).unwrap();
        let decoded: ConsoleCommand = serde_json::from_str(&encoded).unwrap();

        assert!(matches!(
            decoded,
            ConsoleCommand::SyncWalletAssets { wallet_address }
                if wallet_address == "0xwithdrawal"
        ));
    }
}
