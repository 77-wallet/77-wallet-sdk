pub mod endpoint {
    pub mod multisig {
        // Agree to participate in a multisig account
        pub const SIGNED_ORDER_ACCEPT: &str = "signed/order/accept";

        // Initiator cancels a multisig account
        pub const SIGNED_ORDER_CANCEL: &str = "signed/order/cancel";

        // Multisig deployment: Report the service fee hash to the backend
        pub const SIGNED_ORDER_UPDATE_RECHARGE_HASH: &str = "signed/order/updateRechargeHash";

        // Multisig deployment: Report the deployment transaction hash to the backend
        pub const SIGNED_ORDER_UPDATE_SIGNED_HASH: &str = "signed/order/updateSignedHash";

        // Create a multisig transaction queue
        // pub const SIGNED_TRAN_CREATE: &str = "signed/trans/create";
        pub const SIGNED_TRAN_CREATE: &str = "signed/trans/v2/create";

        // Sign a multisig transaction
        pub const SIGNED_TRAN_ACCEPT: &str = "signed/trans/accept";

        // Report the transaction hash of the executed multisig to the backend
        pub const SIGNED_TRAN_UPDATE_TRANS_HASH: &str = "signed/trans/updateTransdHash";

        pub const PERMISSION_ACCEPT: &str = "permission/change";

        // report raw_data to backend
        pub const SIGNED_ORDER_SAVE_RAW_DATA: &str = "signed/order/saveRawData";
    }

    pub const UPLOAD_PERMISSION_TRANS: &str = "permission/uploadTrans";

    pub const DEVICE_INIT: &str = "device/init";
    pub const DEVICE_QUERY_SEND_MSG_AMOUNT: &str = "device/querySendMsgAmount";
    pub const DEVICE_DELETE: &str = "device/delete";
    /// 这个接口现在后端什么也不做
    pub const DEVICE_UNBIND_ADDRESS: &str = "device/unBindAddress";
    pub const DEVICE_BIND_ADDRESS: &str = "device/bindAddress";
    // pub const KEYS_INIT: &str = "keys/init";
    pub const KEYS_V2_INIT: &str = "keys/v2/init";
    pub const KEYS_UPDATE_WALLET_NAME: &str = "keys/updateWalletName";
    pub const KEYS_RESET: &str = "keys/reset";

    // pub const ADDRESS_INIT: &str = "address/init";
    pub const ADDRESS_BATCH_INIT: &str = "address/batch/init";
    pub const ADDRESS_UPDATE_ACCOUNT_NAME: &str = "address/updateAccountName";
    pub const LANGUAGE_INIT: &str = "language/init";

    pub const TOKEN_CUSTOM_TOKEN_INIT: &str = "token/custom/token/init";
    pub const TOKEN_QUERY_RATES: &str = "token/queryRates";
    pub const SYS_CONFIG_FIND_CONFIG_BY_KEY: &str = "sys/config/findConfigByKey";
    pub const APP_INSTALL_DOWNLOAD: &str = "app/install/download";
    pub const APP_INSTALL_SAVE: &str = "app/install/save";
    pub const VERSION_VIEW: &str = "version/view";
    // pub const CHAIN_DEFAULT_LIST: &str = "chain/defaultList";
    pub const CHAIN_LIST: &str = "chain/v2/list";
    pub const CHAIN_RPC_LIST: &str = "chain/rpcList";
    pub const MQTT_INIT: &str = "mqtt/init";

    pub const SEND_MSG_CONFIRM: &str = "sendMsg/confirm";

    pub const VERSION_DOWNLOAD: &str = "version/download";

    pub const ADDRESS_FIND_MULTI_SIGNED_DETAILS: &str = "address/findMultiSignedDetails";
    pub const DEVICE_EDIT_DEVICE_INVITEE_STATUS: &str = "device/editDeviceInviteeStatus";
    pub const DEVICE_UPDATE_APP_ID: &str = "device/updateAppId";

    pub const CLIENT_TASK_LOG_UPLOAD: &str = "client/taskLog/upload";
    pub const TOKEN_BALANCE_REFRESH: &str = "token/balance/refresh";

    //  swap 相关的交易
    pub const SWAP_APPROVE_CANCEL: &str = "swap/approve/cancel";
    pub const SWAP_APPROVE_SAVE: &str = "swap/approve/save";

    pub mod old_wallet {
        pub const OLD_KEYS_V2_INIT: &str = "owallet/keys/v2/init";
        pub const OLD_ADDRESS_BATCH_INIT: &str = "owallet/address/batch/init";
        /// uid 检查
        pub const OLD_KEYS_UID_CHECK: &str = "owallet/keys/uidCheck";

        pub const OLD_CHAIN_RPC_LIST: &str = "owallet/chain/rpcList";
    }

    pub mod api_wallet {
        /// 上报打手续费
        pub const TRANS_SERVICE_FEE_TRANS: &str = "awallet/aw/trans/serviceFeeTrans";
        /// 上报执行结果
        pub const TRANS_EXECUTE_COMPLETE: &str = "awallet/aw/trans/executeComplete";
        /// 收到交易事件确认
        pub const TRANS_EVENT_ACK: &str = "awallet/aw/trans/eventAck";

        pub const TRANS_AUDIT: &str = "awallet/aw/trans/audit";

        /// API钱包消息确认
        pub const MSG_ACK: &str = "awallet/aw/msg/ack";
        // API钱包消息未确认重发
        pub const MSG_ACK_EXPIRED_RESEND: &str = "awallet/aw/msg/ackExpired/resend";
        /// 查询uid的地址列表
        pub const QUERY_ADDRESS_LIST: &str = "awallet/aw/address/list";

        /// 地址初始化
        pub const ADDRESS_INIT: &str = "awallet/aw/address/init";
        /// 地址初始化
        pub const ADDRESS_EXPAND_COMPLETE: &str = "awallet/aw/address/expand/complete";
        pub const INIT_SWAP: &str = "awallet/aw/init/swap";
        /// 设置UID为API钱包
        pub const INIT_API_WALLET: &str = "awallet/aw/init/apiWallet";

        /// UID绑定appId
        pub const APP_ID_BIND: &str = "awallet/aw/appid/bind";
        /// UID解绑appId
        pub const APP_ID_UNBIND: &str = "awallet/aw/appid/unbind";
        /// 保存钱包激活配置
        pub const SAVE_WALLET_ACTIVATION_CONFIG: &str = "awallet/aw/appid/saveActiveInfo";
        /// 查询钱包激活信息
        pub const QUERY_WALLET_ACTIVATION_CONFIG: &str = "awallet/aw/appid/getActiveInfo";
        /// 查询uid绑定信息
        pub const QUERY_UID_BIND_INFO: &str = "awallet/aw/appid/bindInfo";

        pub const APP_CONFIG: &str = "awallet/aw/appid/appConfig";
        // /// Uid与Appid的绑定
        pub const APPID_WITHDRAWAL_WALLET_CHANGE: &str = "awallet/aw/appid/wdWallet/change";

        // /// 导入子账户钱包
        pub const APPID_IMPORT_RECHARGE_WALLET: &str = "awallet/aw/appid/rechargeWallet/import";
        /// 导入钱包
        pub const APPID_IMPORT_WALLET: &str = "awallet/aw/appid/import";
        /// 导入钱包
        pub const APPID_UID_USAGE: &str = "awallet/aw/appid/uid/usage";
        // /// 绑定子账户钱包
        // pub const APPID_SUB_ACCOUNT_BIND: &str = "awallet/aw/appid/rechargeWallet/bind";

        /// 提币策略保存
        pub const TRANS_STRATEGY_WITHDRAWAL_SAVE: &str = "awallet/aw/strategy/withdrawal/save";
        /// 获取提币策略
        pub const TRANS_STRATEGY_GET_WITHDRAWAL_CONFIG: &str =
            "awallet/aw/strategy/getWithdrawalConfig";
        /// 归集策略保存
        pub const TRANS_STRATEGY_COLLECT_SAVE: &str = "awallet/aw/strategy/collect/save";
        /// 获取归集策略
        pub const TRANS_STRATEGY_GET_COLLECT_CONFIG: &str = "awallet/aw/strategy/getCollectConfig";

        /// api钱包查询链列表
        pub const API_WALLET_CHAIN_LIST: &str = "awallet/aw/chain/list";
        /// api钱包查询配置
        pub const API_WALLET_CONFIG: &str = "awallet/aw/appid/configs";
        /// 查询链下index的资产
        pub const QUERY_ASSET_LIST: &str = "awallet/aw/address/assetList";
        /// api钱包查询代币列表
        pub const API_WALLET_COIN_LIST: &str = "awallet/aw/token/queryByPage";

        /// SDK申请资源委托
        pub const RESOURCE_DELEGATION_APPLY: &str = "awallet/aw/trans/resourceDl/apply";
    }
}

/// 代理的全局rpc节点
pub const BASE_RPC_URL: &str = "apprpc.88ai.fun";

cfg_if::cfg_if! {
    // 默认使用开发环境 (dev)
    if #[cfg(any(feature = "dev", not(any(feature = "test", feature = "prod"))))] {
        pub const BASE_URL: &str = "https://walletapi.awsservice.xyz";
    // 测试环境
    } else if #[cfg(feature = "test")] {
        pub const BASE_URL: &str = "https://test-api.awsservice.xyz";

    // 生产环境
    } else if #[cfg(feature = "prod")] {
        pub const BASE_URL: &str = "https://api.77wallet.org";
    } else {
        compile_error!("No valid feature selected! Use 'dev', 'test', or 'prod'.");
    }
}
