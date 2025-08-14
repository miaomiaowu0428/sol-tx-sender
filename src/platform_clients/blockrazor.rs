use std::fmt;
impl fmt::Display for Blockrazor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Blockrazor")
    }
}use base64::Engine;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::{signature::Signature, transaction::Transaction};

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{BuildBundle, BuildTx, Region, SendBundle, SendTxEncoded};

const BLOCKRAZOR_TIP_ACCOUNTS: &[Pubkey] = &[
    // pubkey!("FjmZZrFvhnqqb9ThCuMVnENaM3JGVuGWNyCAxRJcFpg9"),
    pubkey!("6No2i3aawzHsjtThw81iq1EXPJN6rh8eSJCLaYZfKDTG"),
    // pubkey!("A9cWowVAiHe9pJfKAj3TJiN9VpbzMUq6E4kEvf5mUT22"),
    // pubkey!("Gywj98ophM7GmkDdaWs4isqZnDdFCW7B46TXmKfvyqSm"),
    // pubkey!("68Pwb4jS7eZATjDfhmTXgRJjCiZmw1L7Huy4HNpnxJ3o"),
    // pubkey!("4ABhJh5rZPjv63RBJBuyWzBK3g9gWMUQdTZP2kiW31V9"),
    // pubkey!("B2M4NG5eyZp5SBQrSdtemzk5TqVuaWGQnowGaCBt8GyM"),
    // pubkey!("5jA59cXMKQqZAVdtopv8q3yyw9SYfiE3vUCbt7p8MfVf"),
    // pubkey!("5YktoWygr1Bp9wiS1xtMtUki1PeYuuzuCF98tqwYxf61"),
    // pubkey!("295Avbam4qGShBYK7E9H5Ldew4B3WyJGmgmXfiWdeeyV"),
    // pubkey!("EDi4rSy2LZgKJX74mbLTFk4mxoTgT6F7HxxzG2HBAFyK"),
    // pubkey!("BnGKHAC386n4Qmv9xtpBVbRaUTKixjBe3oagkPFKtoy6"),
    // pubkey!("Dd7K2Fp7AtoN8xCghKDRmyqr5U169t48Tw5fEd3wT9mq"),
    // pubkey!("AP6qExwrbRgBAVaehg4b5xHENX815sMabtBzUzVB4v8S"),
];

const BLOCKRAZOR_ENDIPOINTS: &[&str] = &[
    "http://frankfurt.solana.blockrazor.xyz:443/sendTransaction", //Frankfurt
    "http://newyork.solana.blockrazor.xyz:443/sendTransaction",   // NewTork
    "http://tokyo.solana.blockrazor.xyz:443/sendTransaction",     // Tokyo
    "http://amsterdam.solana.blockrazor.xyz:443/sendTransaction", // Amsterdam
];

pub struct Blockrazor {
    pub endpoint: String,
    pub region: Region,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl Blockrazor {
    const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip

    pub fn new() -> Self {
        Self::with_client(HTTP_CLIENT.clone())
    }

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::Frankfurt => BLOCKRAZOR_ENDIPOINTS[0].to_string(),
            Region::NewYork => BLOCKRAZOR_ENDIPOINTS[1].to_string(),
            Region::Tokyo => BLOCKRAZOR_ENDIPOINTS[2].to_string(),
            Region::Amsterdam => BLOCKRAZOR_ENDIPOINTS[3].to_string(),
            _ => BLOCKRAZOR_ENDIPOINTS[0].to_string(),
        }
    }

    pub fn with_client(http_client: Arc<Client>) -> Self {
        let region = read_region_from_env();
        let endpoint = match region {
            Region::Frankfurt => BLOCKRAZOR_ENDIPOINTS[0].to_string(),
            Region::NewYork => BLOCKRAZOR_ENDIPOINTS[1].to_string(),
            Region::Tokyo => BLOCKRAZOR_ENDIPOINTS[2].to_string(),
            Region::Amsterdam => BLOCKRAZOR_ENDIPOINTS[3].to_string(),
            _ => BLOCKRAZOR_ENDIPOINTS[0].to_string(),
        };
        let auth_token = read_auth_token_from_env();
        Blockrazor {
            endpoint,
            region,
            auth_token,
            http_client,
        }
    }

    pub fn get_tip_address(&self) -> Pubkey {
        *BLOCKRAZOR_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| BLOCKRAZOR_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

// 占位函数，实际可用 std::env::var("BLOCKRAZOR_REGION")
fn read_region_from_env() -> Region {
    Region::Frankfurt
}
fn read_auth_token_from_env() -> String {
    std::env::var("BLOCKRAZOR_KEY").unwrap_or_default()
}

// 新模式实现

#[async_trait::async_trait]
impl SendTxEncoded for Blockrazor {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("apikey", self.auth_token.as_str())
            .json(&json!({
                "transaction": tx_base64,
                "mode": "fast"
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
        log::info!("{:?}", response);
        Ok(())
    }
}

#[async_trait::async_trait]
impl SendBundle for Blockrazor {
    async fn send_bundle(&self, _txs: &[Transaction]) -> Result<Vec<Signature>, String> {
        Err("Blockrazor 暂不支持批量交易".to_string())
    }
}

impl BuildTx for Blockrazor {
    fn get_tip_address(&self) -> Pubkey {
        self.get_tip_address()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}

impl BuildBundle for Blockrazor {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, Blockrazor> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}
