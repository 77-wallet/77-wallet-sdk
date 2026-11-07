use std::time::{Duration, Instant};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use wallet_database::{ApiWalletDbPool, repositories::api_wallet::assets::ApiAssetsRepo};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn arg_parse<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    arg_value(args, name).and_then(|v| v.parse::<T>().ok())
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "Usage: cargo run -p wallet-database --bin assets_sql_bench -- \\\n\
         \t--db /path/to/api_wallet.db \\\n\
         \t--wallet 0x... \\\n\
         \t[--account-id 123] [--chain-code ETH] [--iters 10] [--warmup 2]\n"
    );
    std::process::exit(2);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let db = arg_value(&args, "--db").unwrap_or_else(|| {
        "/Users/apple/Work/rust/77_wallet_sdk/wallet-api/test_data/db/api_wallet.db".to_string()
    });
    let wallet = arg_value(&args, "--wallet").unwrap_or_else(|| print_usage_and_exit());
    let account_id = arg_parse::<u32>(&args, "--account-id");
    let chain_code = arg_value(&args, "--chain-code");
    let iters = arg_parse::<usize>(&args, "--iters").unwrap_or(10);
    let warmup = arg_parse::<usize>(&args, "--warmup").unwrap_or(2);

    let opts = SqliteConnectOptions::from_str(&db)
        .unwrap_or_else(|e| panic!("Invalid db path: {db}, err: {e:?}"))
        .read_only(true);

    let raw_pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .unwrap_or_else(|e| panic!("Failed to connect db: {db}, err: {e:?}"));
    let pool = ApiWalletDbPool::new(std::sync::Arc::new(raw_pool));

    println!("DB: {db}");
    println!("wallet: {wallet}");
    println!("account_id: {:?}", account_id);
    println!("chain_code: {:?}", chain_code.as_deref());
    println!("warmup: {warmup}, iters: {iters}");
    println!("------------------------------------");

    for _ in 0..warmup {
        let _ = ApiAssetsRepo::get_api_wallet_total_assets_v2(
            &pool,
            Some(&wallet),
            account_id,
            chain_code.as_deref(),
        )
        .await;

        let _ = ApiAssetsRepo::get_api_wallet_total_assets_v3(
            &pool,
            &wallet,
            account_id,
            chain_code.as_deref(),
        )
        .await;
    }

    let mut v2_times: Vec<Duration> = Vec::with_capacity(iters);
    let mut v3_times: Vec<Duration> = Vec::with_capacity(iters);

    for i in 0..iters {
        let start = Instant::now();
        let v2 = ApiAssetsRepo::get_api_wallet_total_assets_v2(
            &pool,
            Some(&wallet),
            account_id,
            chain_code.as_deref(),
        )
        .await;
        let d = start.elapsed();
        v2_times.push(d);
        println!("[{}/{}] v2: {:?} ({})", i + 1, iters, d, if v2.is_ok() { "ok" } else { "err" });

        let start = Instant::now();
        let v3 = ApiAssetsRepo::get_api_wallet_total_assets_v3(
            &pool,
            &wallet,
            account_id,
            chain_code.as_deref(),
        )
        .await;
        let d = start.elapsed();
        v3_times.push(d);
        println!("[{}/{}] v3: {:?} ({})", i + 1, iters, d, if v3.is_ok() { "ok" } else { "err" });
    }

    fn stats(mut xs: Vec<Duration>) -> (Duration, Duration, Duration) {
        xs.sort_unstable();
        let min = xs.first().copied().unwrap_or_default();
        let max = xs.last().copied().unwrap_or_default();
        let p50 = xs.get(xs.len() / 2).copied().unwrap_or_default();
        (min, p50, max)
    }

    let (v2_min, v2_p50, v2_max) = stats(v2_times);
    let (v3_min, v3_p50, v3_max) = stats(v3_times);

    println!("====================================");
    println!("v2 min/p50/max: {:?} / {:?} / {:?}", v2_min, v2_p50, v2_max);
    println!("v3 min/p50/max: {:?} / {:?} / {:?}", v3_min, v3_p50, v3_max);
}
