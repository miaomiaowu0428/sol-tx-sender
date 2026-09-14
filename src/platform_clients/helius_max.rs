use std::fmt;
impl fmt::Display for HeliusMax {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HeliusMax")
    }
}
use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use utils::log_time;

use solana_sdk::{pubkey, pubkey::Pubkey};
use solana_sdk::signature::Signature;

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region, TxExt};
use solana_sdk::transaction::VersionedTransaction;

// helius (Sender Max) 小费地址
pub const HELIUS_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE"),
    pubkey!("D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ"),
    pubkey!("9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta"),
    pubkey!("5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn"),
    pubkey!("2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD"),
    pubkey!("2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ"),
    pubkey!("wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF"),
    pubkey!("3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT"),
    pubkey!("4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey"),
    pubkey!("4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or"),
];

// helius (Sender Max) 地址 — 全部路径路由，无 swqos_only 参数
pub const HELIUS_ENDPOINT: &[&str] = &[
    "http://ewr-sender.helius-rpc.com/fast", // NY
    "http://ams-sender.helius-rpc.com/fast", // Amsterdam
    "http://fra-sender.helius-rpc.com/fast", // Frankfurt
    "http://lon-sender.helius-rpc.com/fast", // London
    "http://slc-sender.helius-rpc.com/fast", // Salt Lake City
    "http://tyo-sender.helius-rpc.com/fast", // Tokyo
    "http://sg-sender.helius-rpc.com/fast",  // Singapore
];

/// Helius **Sender Max** 平台。
///
/// 最低 tip 0.001 SOL，路由到全部高吞吐路径，适用于需要最快落地的交易。
/// 端点为 `.../fast`（无查询参数），支持单笔 + bundle（`sendBundle`）。
#[derive(Clone)]
pub struct HeliusMax {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl HeliusMax {
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_001_000_000; // 单笔交易最低 tip（Max，0.001 SOL）
    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => HELIUS_ENDPOINT[0].to_string(),
            Region::Amsterdam => HELIUS_ENDPOINT[1].to_string(),
            Region::Frankfurt => HELIUS_ENDPOINT[2].to_string(),
            Region::London => HELIUS_ENDPOINT[3].to_string(),
            Region::SaltLakeCity => HELIUS_ENDPOINT[4].to_string(),
            Region::Tokyo => HELIUS_ENDPOINT[5].to_string(),
            Region::Singapore => HELIUS_ENDPOINT[6].to_string(),
            _ => HELIUS_ENDPOINT[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => HELIUS_ENDPOINT[0].to_string(),
            Region::Amsterdam => HELIUS_ENDPOINT[1].to_string(),
            Region::Frankfurt => HELIUS_ENDPOINT[2].to_string(),
            Region::London => HELIUS_ENDPOINT[3].to_string(),
            Region::SaltLakeCity => HELIUS_ENDPOINT[4].to_string(),
            Region::Tokyo => HELIUS_ENDPOINT[5].to_string(),
            Region::Singapore => HELIUS_ENDPOINT[6].to_string(),
            _ => HELIUS_ENDPOINT[0].to_string(),
        };
        let auth_token = std::env::var("HELIUS_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        HeliusMax {
            endpoint,
            auth_token,
            http_client,
        }
    }

    /// 显式构造：调用方负责提供 key 和 region，不读取任何环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        let endpoint = match region {
            Region::NewYork => HELIUS_ENDPOINT[0].to_string(),
            Region::Amsterdam => HELIUS_ENDPOINT[1].to_string(),
            Region::Frankfurt => HELIUS_ENDPOINT[2].to_string(),
            Region::London => HELIUS_ENDPOINT[3].to_string(),
            Region::SaltLakeCity => HELIUS_ENDPOINT[4].to_string(),
            Region::Tokyo => HELIUS_ENDPOINT[5].to_string(),
            Region::Singapore => HELIUS_ENDPOINT[6].to_string(),
            _ => HELIUS_ENDPOINT[0].to_string(),
        };
        HeliusMax {
            endpoint,
            auth_token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for HeliusMax {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("helius(max) send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
            let res = self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .header("api-key", self.auth_token.as_str())
                .json(&json!({
                    "id": 1,
                    "jsonrpc": "2.0",
                    "method": "sendTransaction",
                    "params": [
                        tx_base64,
                        {
                            "encoding": "base64",
                            "skipPreflight": true,
                            "maxRetries": 0,
                        }
                    ],
                }))
                .send()
                .await;
            let response = match res {
                Ok(resp) => match resp.text().await {
                    Ok(text) => text,
                    Err(e) => return Err(format!("response text error: {}", e)),
                },
                Err(e) => {
                    log::error!("send error: {:?}", e);
                    return Err(format!("send error: {}", e));
                }
            };
            info!("helius(max): {}", response);
            Ok(())
        })
    }
}

impl crate::platform_clients::BuildTx for HeliusMax {
    fn get_tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::HeliusMax
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    fn tip_recvs(&self) -> Vec<Pubkey> {
        HELIUS_TIP_ACCOUNTS.to_vec()
    }
    // Helius 走 Sender 的纯 tip 缓冲，不做 cu_price 竞价：忽略任何 cu.price，不加 price 指令
    fn uses_cu_price(&self) -> bool {
        false
    }
    // 使用默认实现，无需重写 build_tx
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for HeliusMax {
    /// Sender Max `sendBundle`：直接 POST 到 `/fast` 端点，最多 4 笔，base64 编码。
    ///
    /// 按官方要求：至少一笔交易需带 ≥0.001 SOL 的 Sender tip，每笔需带 priority fee
    /// （由 `BundleBuilder::append` 的 tip / cu 参数控制）。追踪按交易签名，不用 bundle id。
    async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<Vec<Signature>, String> {
        if txs.is_empty() || txs.len() > 4 {
            return Err(format!(
                "HeliusMax sendBundle requires 1-4 transactions, got {}",
                txs.len()
            ));
        }
        log_time!("helius(max) sendBundle: ", {
            let mut encoded_txs = Vec::with_capacity(txs.len());
            let mut sigs: Vec<Signature> = Vec::with_capacity(txs.len());
            for tx in txs {
                let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
                encoded_txs.push(tx_base64);
                sigs.push(tx.sig());
            }
            let res = self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .header("api-key", self.auth_token.as_str())
                .json(&json!({
                    "id": 1,
                    "jsonrpc": "2.0",
                    "method": "sendBundle",
                    "params": [
                        encoded_txs,
                        { "encoding": "base64" }
                    ],
                }))
                .send()
                .await;
            let response = match res {
                Ok(resp) => match resp.text().await {
                    Ok(text) => text,
                    Err(e) => return Err(format!("response text error: {}", e)),
                },
                Err(e) => {
                    log::error!("helius(max) sendBundle send error: {:?}", e);
                    return Err(format!("send error: {}", e));
                }
            };
            info!("helius(max) sendBundle: {}", response);
            // 响应含 error 字段（如 tip 不足被拒）时视为失败
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&response) {
                if let Some(err) = v.get("error") {
                    return Err(format!("helius(max) sendBundle error: {}", err));
                }
            }
            Ok(sigs)
        })
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::BundleSender for HeliusMax {
    fn tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn max_tx_size(&self) -> usize {
        // V1 交易上限 4096 字节（SIMD-0296）。V0 交易实际不会超过此值。
        4096
    }
    // Helius bundle 同样走纯 tip 缓冲，忽略 cu.price、不加 price 指令
    fn uses_cu_price(&self) -> bool {
        false
    }
    async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<Vec<Signature>, String> {
        <Self as crate::platform_clients::SendBundle>::send_bundle(self, txs).await
    }
}
