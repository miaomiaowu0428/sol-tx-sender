use base64::Engine;
use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::{signature::Signature, transaction::Transaction};

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::Region;

pub const ZEROSLOT_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("6fQaVhYZA4w3MBSXjJ81Vf6W1EDYeUPXpgVQ6UQyU1Av"),
    pubkey!("4HiwLEP2Bzqj3hM2ENxJuzhcPCdsafwiet3oGkMkuQY4"),
    pubkey!("7toBU3inhmrARGngC7z6SjyP85HgGMmCTEwGNRAcYnEK"),
    pubkey!("8mR3wB1nh4D6J9RUCugxUpc6ya8w38LPxZ3ZjcBhgzws"),
    pubkey!("6SiVU5WEwqfFapRuYCndomztEwDjvS5xgtEof3PLEGm9"),
    pubkey!("TpdxgNJBWZRL8UXF5mrEsyWxDWx9HQexA9P1eTWQ42p"),
    pubkey!("D8f3WkQu6dCF33cZxuAsrKHrGsqGP2yvAHf8mX6RXnwf"),
    pubkey!("GQPFicsy3P3NXxB5piJohoxACqTvWE9fKpLgdsMduoHE"),
    pubkey!("Ey2JEr8hDkgN8qKJGrLf2yFjRhW7rab99HVxwi5rcvJE"),
    pubkey!("4iUgjMT8q2hNZnLuhpqZ1QtiV8deFPy2ajvvjEpKKgsS"),
    pubkey!("3Rz8uD83QsU8wKvZbgWAPvCNDU6Fy8TSZTMcPm3RB6zt"),
];

pub const ZEROSLOT_ENDPOINT: &[&str] = &[
    "https://ny.0slot.trade",  // NewYork
    "https://de.0slot.trade",  // Frankfurt
    "https://ams.0slot.trade", // Amsterdam
    "https://jp.0slot.trade",  // Tokyo
    "https://la.0slot.trade",  // LosAngeles
];

pub struct ZeroSlot {
    pub endpoint: String,
    pub token: String,
    pub http_client: Arc<Client>,
}

impl ZeroSlot {
    const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => ZEROSLOT_ENDPOINT[0].to_string(),
            Region::Frankfurt => ZEROSLOT_ENDPOINT[1].to_string(),
            Region::Amsterdam => ZEROSLOT_ENDPOINT[2].to_string(),
            Region::Tokyo => ZEROSLOT_ENDPOINT[3].to_string(),
            Region::LosAngeles => ZEROSLOT_ENDPOINT[4].to_string(),
            _ => String::new(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => ZEROSLOT_ENDPOINT[0].to_string(),
            Region::Frankfurt => ZEROSLOT_ENDPOINT[1].to_string(),
            Region::Amsterdam => ZEROSLOT_ENDPOINT[2].to_string(),
            Region::Tokyo => ZEROSLOT_ENDPOINT[3].to_string(),
            Region::LosAngeles => ZEROSLOT_ENDPOINT[4].to_string(),
            _ => ZEROSLOT_ENDPOINT[0].to_string(),
        };
        let token = std::env::var("ZEROSLOT_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        ZeroSlot {
            endpoint,
            token,
            http_client,
        }
    }

    // 随机获取一个tip地址
    fn get_tip_address() -> Pubkey {
        *ZEROSLOT_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ZEROSLOT_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for ZeroSlot {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let mut url = String::new();
        url.push_str(&self.endpoint);
        url.push_str("?api-key=");
        url.push_str(&self.token);
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&json! ({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendTransaction",
                "params": [
                    tx_base64,
                    {
                        "encoding": "base64",
                        "skipPreflight": true,
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
        info!("zeroslot: {}", response);
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for ZeroSlot {
    async fn send_bundle(&self, txs: &[Transaction]) -> Result<Vec<Signature>, String> {
        let mut sigs = Vec::new();
        for tx in txs {
            let encode_txs = match bincode::serialize(tx) {
                Ok(bytes) => base64::prelude::BASE64_STANDARD.encode(&bytes),
                Err(e) => return Err(format!("bincode serialize error: {}", e)),
            };
            let mut url = String::new();
            url.push_str(&self.endpoint);
            url.push_str("?api-key=");
            url.push_str(&self.token);
            let res = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&json! ({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "sendTransaction",
                    "params": [
                        encode_txs,
                        {
                            "encoding": "base64",
                            "skipPreflight": true,
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
            log::info!("zeroslot: {}", response);
            sigs.push(tx.signatures[0]);
        }
        Ok(sigs)
    }
}

impl crate::platform_clients::BuildTx for ZeroSlot {
    fn get_tip_address(&self) -> Pubkey {
        *ZEROSLOT_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ZEROSLOT_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for ZeroSlot {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, ZeroSlot> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}

impl std::fmt::Display for ZeroSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ZeroSlot")
    }
}
