use std::fmt;
impl fmt::Display for HeliusSwqos {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HeliusSwqos")
    }
}
use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use utils::log_time;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region, TxExt};
use solana_sdk::transaction::VersionedTransaction;

// helius (Sender SWQOS-only) 小费地址（与 Max 共用的 Sender tip account）
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

// helius (Sender SWQOS-only) 地址 — 单条 SWQOS 路径，需带 swqos_only=true
pub const HELIUS_ENDPOINT: &[&str] = &[
    "http://ewr-sender.helius-rpc.com/fast?swqos_only=true", // NY
    "http://ams-sender.helius-rpc.com/fast?swqos_only=true", // Amsterdam
    "http://fra-sender.helius-rpc.com/fast?swqos_only=true", // Frankfurt
    "http://lon-sender.helius-rpc.com/fast?swqos_only=true", // London
    "http://slc-sender.helius-rpc.com/fast?swqos_only=true", // Salt Lake City
    "http://tyo-sender.helius-rpc.com/fast?swqos_only=true", // Tokyo
    "http://sg-sender.helius-rpc.com/fast?swqos_only=true",  // Singapore
];

/// Helius **Sender SWQOS-only** 平台。
///
/// 最低 tip 0.000005 SOL，只走单条 SWQOS 低成本路径，适用于大批量低成本交易。
/// 端点为 `.../fast?swqos_only=true`。该档位**不支持 bundle**（仅单笔 `sendTransaction`），
/// 因此只实现 `SendTx` / `BuildTx`，不实现 `SendBundle` / `BundleSender`。
#[derive(Clone)]
pub struct HeliusSwqos {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl HeliusSwqos {
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_000_005_000; // 单笔交易最低 tip（SWQOS-only，0.000005 SOL）
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
        HeliusSwqos {
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
        HeliusSwqos {
            endpoint,
            auth_token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for HeliusSwqos {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("helius(swqos) send: ", {
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
            info!("helius(swqos): {}", response);
            Ok(())
        })
    }
}

impl crate::platform_clients::BuildTx for HeliusSwqos {
    fn get_tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::HeliusSwqos
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    fn tip_recvs(&self) -> Vec<Pubkey> {
        HELIUS_TIP_ACCOUNTS.to_vec()
    }
    // SWQOS-only 档位只走单条低成本路径、不做 cu_price 竞价：忽略任何 cu.price，不加 price 指令
    fn uses_cu_price(&self) -> bool {
        false
    }
    // SWQOS-only 单笔 tip 最高不超过 0.0002 SOL（200_000 lamports）
    fn max_tip_amount(&self) -> Option<u64> {
        Some(200_000)
    }
    // 使用默认实现，无需重写 build_tx
}
