#!/usr/bin/env rust

use std::time::Instant;
use wallet_database::{ApiWalletDbPool, repositories::api_wallet::assets::ApiAssetsRepo};

#[tokio::main]
async fn main() {
    // 初始化数据库连接
    let pool = ApiWalletDbPool::connect("sqlite:./wallet.db").await
        .expect("Failed to connect to database");
    
    let wallet_address = "0x1234567890123456789012345678901234567890";
    
    println!("Testing performance for wallet: {}", wallet_address);
    println!("====================================");
    
    // 测试 v2 接口
    println!("Testing v2 interface...");
    let start_v2 = Instant::now();
    match ApiAssetsRepo::get_api_wallet_total_assets_v2(&pool, Some(wallet_address), None, None).await {
        Ok(result) => {
            let duration_v2 = start_v2.elapsed();
            println!("v2 result: total_amount={}, total_coins_quantity={}", result.total_amount, result.total_coins_quantity);
            println!("v2 time: {:?}", duration_v2);
        }
        Err(e) => {
            println!("v2 error: {:?}", e);
        }
    }
    
    println!("------------------------------------");
    
    // 测试 v3 接口
    println!("Testing v3 interface...");
    let start_v3 = Instant::now();
    match ApiAssetsRepo::get_api_wallet_total_assets_v3(&pool, wallet_address).await {
        Ok(assets) => {
            let duration_v3 = start_v3.elapsed();
            println!("v3 result: {} assets found", assets.len());
            println!("v3 time: {:?}", duration_v3);
        }
        Err(e) => {
            println!("v3 error: {:?}", e);
        }
    }
    
    println!("====================================");
    println!("Test completed!");
}