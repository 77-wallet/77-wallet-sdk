use anyhow::{Context as _, Result, ensure};
use wallet_transport::{client::RpcClient, types::JsonRpcParams};

const READ_ONLY_ADDRESS: &str = "0x000000000000000000000000000000000000dEaD";

struct EvmLiveChain {
    name: &'static str,
    env: &'static str,
    default_rpc: &'static str,
    expected_chain_id: u64,
}

fn parse_quantity(value: &str) -> Result<u64> {
    let raw = value.strip_prefix("0x").context("quantity must start with 0x")?;
    ensure!(!raw.is_empty(), "quantity must not be empty");
    Ok(u64::from_str_radix(raw, 16)?)
}

fn validate_large_quantity(value: &str) -> Result<()> {
    let raw = value.strip_prefix("0x").context("quantity must start with 0x")?;
    ensure!(!raw.is_empty(), "quantity must not be empty");
    ensure!(raw.bytes().all(|byte| byte.is_ascii_hexdigit()), "quantity must be hexadecimal");
    Ok(())
}

async fn no_params(client: &RpcClient, method: &str) -> Result<String> {
    Ok(client
        .invoke_request(JsonRpcParams::<Vec<String>>::default().method(method).params(vec![]))
        .await?)
}

async fn account_quantity(client: &RpcClient, method: &str) -> Result<String> {
    Ok(client
        .invoke_request(
            JsonRpcParams::default().method(method).params(vec![READ_ONLY_ADDRESS, "latest"]),
        )
        .await?)
}

async fn validate_read_only_rpc_matrix(chains: &[EvmLiveChain]) -> Result<()> {
    for chain in chains {
        let rpc = std::env::var(chain.env).unwrap_or_else(|_| chain.default_rpc.to_string());
        let client = RpcClient::new(&rpc, None, Some(std::time::Duration::from_secs(20)))
            .with_context(|| format!("{} RPC client", chain.name))?;

        let chain_id = no_params(&client, "eth_chainId")
            .await
            .with_context(|| format!("{} eth_chainId", chain.name))?;
        ensure!(
            parse_quantity(&chain_id)? == chain.expected_chain_id,
            "{} returned unexpected chain id {}",
            chain.name,
            chain_id
        );

        let block = no_params(&client, "eth_blockNumber")
            .await
            .with_context(|| format!("{} eth_blockNumber", chain.name))?;
        ensure!(parse_quantity(&block)? > 0, "{} latest block must be positive", chain.name);

        let gas_price = no_params(&client, "eth_gasPrice")
            .await
            .with_context(|| format!("{} eth_gasPrice", chain.name))?;
        ensure!(parse_quantity(&gas_price)? > 0, "{} gas price must be positive", chain.name);

        let balance = account_quantity(&client, "eth_getBalance")
            .await
            .with_context(|| format!("{} eth_getBalance", chain.name))?;
        validate_large_quantity(&balance)?;

        let nonce = account_quantity(&client, "eth_getTransactionCount")
            .await
            .with_context(|| format!("{} eth_getTransactionCount", chain.name))?;
        parse_quantity(&nonce)?;
    }

    Ok(())
}

async fn validate_chain_id_matrix(chains: &[EvmLiveChain]) -> Result<()> {
    for chain in chains {
        let rpc = std::env::var(chain.env).unwrap_or_else(|_| chain.default_rpc.to_string());
        let client = RpcClient::new(&rpc, None, Some(std::time::Duration::from_secs(20)))
            .with_context(|| format!("{} RPC client", chain.name))?;
        let chain_id = no_params(&client, "eth_chainId")
            .await
            .with_context(|| format!("{} eth_chainId", chain.name))?;
        ensure!(
            parse_quantity(&chain_id)? == chain.expected_chain_id,
            "{} returned unexpected chain id {}",
            chain.name,
            chain_id
        );
    }

    Ok(())
}

#[tokio::test]
#[ignore = "manual live smoke: contacts public RPC endpoints but never broadcasts"]
async fn added_evm_chains_read_live_nodes_without_funds() -> Result<()> {
    let chains = [
        EvmLiveChain {
            name: "polygon",
            env: "WALLET_SMOKE_POLYGON_RPC",
            default_rpc: "https://polygon.drpc.org",
            expected_chain_id: 137,
        },
        EvmLiveChain {
            name: "avalanche",
            env: "WALLET_SMOKE_AVALANCHE_RPC",
            default_rpc: "https://api.avax.network/ext/bc/C/rpc",
            expected_chain_id: 43_114,
        },
        EvmLiveChain {
            name: "fantom",
            env: "WALLET_SMOKE_FANTOM_RPC",
            default_rpc: "https://rpcapi.fantom.network",
            expected_chain_id: 250,
        },
        EvmLiveChain {
            name: "arbitrum",
            env: "WALLET_SMOKE_ARBITRUM_RPC",
            default_rpc: "https://arb1.arbitrum.io/rpc",
            expected_chain_id: 42_161,
        },
        EvmLiveChain {
            name: "base",
            env: "WALLET_SMOKE_BASE_RPC",
            default_rpc: "https://mainnet.base.org",
            expected_chain_id: 8_453,
        },
        EvmLiveChain {
            name: "scroll",
            env: "WALLET_SMOKE_SCROLL_RPC",
            default_rpc: "https://rpc.scroll.io",
            expected_chain_id: 534_352,
        },
    ];

    validate_read_only_rpc_matrix(&chains).await
}

#[tokio::test]
#[ignore = "manual live smoke: verifies configured public testnet RPCs without broadcasting"]
async fn added_evm_test_config_nodes_report_expected_chain_ids_without_funds() -> Result<()> {
    let chains = [
        EvmLiveChain {
            name: "polygon-amoy",
            env: "WALLET_SMOKE_POLYGON_TESTNET_RPC",
            default_rpc: "https://polygon-amoy.drpc.org",
            expected_chain_id: 80_002,
        },
        EvmLiveChain {
            name: "avalanche-fuji",
            env: "WALLET_SMOKE_AVALANCHE_TESTNET_RPC",
            default_rpc: "https://api.avax-test.network/ext/bc/C/rpc",
            expected_chain_id: 43_113,
        },
        EvmLiveChain {
            name: "fantom-testnet",
            env: "WALLET_SMOKE_FANTOM_TESTNET_RPC",
            default_rpc: "https://fantom-testnet.rpc.thirdweb.com",
            expected_chain_id: 4_002,
        },
        EvmLiveChain {
            name: "arbitrum-sepolia",
            env: "WALLET_SMOKE_ARBITRUM_TESTNET_RPC",
            default_rpc: "https://sepolia-rollup.arbitrum.io/rpc",
            expected_chain_id: 421_614,
        },
        EvmLiveChain {
            name: "base-sepolia",
            env: "WALLET_SMOKE_BASE_TESTNET_RPC",
            default_rpc: "https://sepolia.base.org",
            expected_chain_id: 84_532,
        },
        EvmLiveChain {
            name: "scroll-sepolia",
            env: "WALLET_SMOKE_SCROLL_TESTNET_RPC",
            default_rpc: "https://scroll-sepolia-rpc.publicnode.com",
            expected_chain_id: 534_351,
        },
    ];

    validate_chain_id_matrix(&chains).await
}
