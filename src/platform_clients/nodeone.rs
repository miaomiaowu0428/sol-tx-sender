use std::fmt;
impl fmt::Display for NodeOne {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeOne")
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

pub const NODEONE_TIP_ACCOUNTS: &[Pubkey] = &[
    // pubkey!("node1PqAa3BWWzUnTHVbw8NJHC874zn9ngAkXjgWEej"),
    pubkey!("node1UzzTxAAeBTpfZkQPJXBAqixsbdth11ba1NXLBG"),
    // pubkey!("node1Qm1bV4fwYnCurP8otJ9s5yrkPq7SPZ5uhj3Tsv"),
    // pubkey!("node1PUber6SFmSQgvf2ECmXsHP5o3boRSGhvJyPMX1"),
    // pubkey!("node1AyMbeqiVN6eoQzEAwCA6Pk826hrdqdAHR7cdJ3"),
    // pubkey!("node1YtWCoTwwVYTFLfS19zquRQzYX332hs1HEuRBjC"),
];

// one one 地址
pub const NODEONE_ENDPOINT: &[&str] = &[
    "https://ny.node1.me",  // NY
    "https://fra.node1.me", // Frankfurt
    "https://ams.node1.me", // Amsterdam
];

#[derive(Clone)]
pub struct NodeOne {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl NodeOne {
    const MIN_TIP_AMOUNT_TX: u64 = 2_000_000; // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 2_000_000; // 批量交易最低 tip
        const DEFAULT_TPS:u64 = 5;

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => NODEONE_ENDPOINT[0].to_string(),
            Region::Frankfurt => NODEONE_ENDPOINT[1].to_string(),
            Region::Amsterdam => NODEONE_ENDPOINT[2].to_string(),
            _ => String::new(),
        }
    }


    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => NODEONE_ENDPOINT[0].to_string(),
            Region::Frankfurt => NODEONE_ENDPOINT[1].to_string(),
            Region::Amsterdam => NODEONE_ENDPOINT[2].to_string(),
            _ => NODEONE_ENDPOINT[0].to_string(),
        };
        let auth_token = std::env::var("NODEONE_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        NodeOne {
            endpoint,
            auth_token,
            http_client,
        }
    }

    // 随机获取一个tip地址
    fn get_tip_address() -> Pubkey {
        *NODEONE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| NODEONE_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for NodeOne {
      
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&json! ({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendTransaction",
                "params": [tx_base64],
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
        info!("node1: {}", response);
        Ok(())
    }
}


impl crate::platform_clients::BuildTx for NodeOne {
    fn get_tip_address(&self) -> Pubkey {
        *NODEONE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| NODEONE_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Nodeone
    }
    
    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    
    // 使用默认实现，无需重写 build_tx
}

