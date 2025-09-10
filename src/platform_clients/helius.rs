use std::fmt;
impl fmt::Display for Helius {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Helius")
    }
}
use base64::Engine;
use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::{signature::Signature, transaction::Transaction};

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region};

// helius 小费地址
pub const HELIUS_TIP_ACCOUNTS: &[Pubkey] = &[
    // pubkey!("4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE"),
    pubkey!("D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ"),
    // pubkey!("9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta"),
    // pubkey!("5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn"),
    // pubkey!("2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD"),
    // pubkey!("2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ"),
    // pubkey!("wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF"),
    // pubkey!("3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT"),
    // pubkey!("4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey"),
    // pubkey!("4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or"),
];

// helius 地址
pub const HELIUS_ENDPOINT: &[&str] = &[
    "http://ewr-sender.helius-rpc.com/fast", // NY
    "http://ams-sender.helius-rpc.com/fast", // Amsterdam
    "http://fra-sender.helius-rpc.com/fast", // Frankfurt
    "http://lon-sender.helius-rpc.com/fast", // London
    "http://slc-sender.helius-rpc.com/fast", // Salt Lake City
    "http://tyo-sender.helius-rpc.com/fast", // Tokyo
    "http://sg-sender.helius-rpc.com/fast",  // Singapore
];

#[derive(Clone)]
pub struct Helius {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl Helius {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip  
    pub const DEFAULT_TPS: u64 = 6;

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
        Helius {
            endpoint,
            auth_token,
            http_client,
        }
    }

    fn get_tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for Helius {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
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
        info!("helius: {}", response);
        Ok(())
    }
}

impl crate::platform_clients::BuildTx for Helius {
    fn get_tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Helius
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}
