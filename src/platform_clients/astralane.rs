use std::fmt;
impl fmt::Display for Astralane {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Astralane")
    }
}

/// Astralane sendBundle 响应结构体
#[derive(Debug, serde::Deserialize)]
struct AstralaneSendBundleResponse {
    pub jsonrpc: Option<String>,
    pub result: Option<Vec<String>>,
    pub error: Option<serde_json::Value>,
    pub id: Option<u64>,
}

use base64::Engine;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::signature::Signature;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region};

pub const ASTRALANE_TIP_ACCOUNTS: &[Pubkey] = &[
    // pubkey!("astrazznxsGUhWShqgNtAdfrzP2G83DzcWVJDxwV9bF"),
    pubkey!("astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm"),
    // pubkey!("astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk"),
    // pubkey!("astraRVUuTHjpwEVvNBeQEgwYx9w9CFyfxjYoobCZhL"),
];

pub const ASTRALANE_ENDPOINTS: &[&str] = &[
    "http://fr.gateway.astralane.io/iris",  // Frankfurt
    "http://lax.gateway.astralane.io/iris", // San Fransisco
    "http://jp.gateway.astralane.io/iris",  // Tokyo
    "http://ny.gateway.astralane.io/iris",  // NewYork
    "http://ams.gateway.astralane.io/iris", // Amsterdam
];

#[derive(Clone)]
pub struct Astralane {
    pub endpoint: String,   // 只保存基础 endpoint，不拼 key
    pub auth_token: String, // 单独保存 key
    pub http_client: Arc<Client>,
}

impl Astralane {
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_000_100_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 0_000_100_000; // 批量交易最低 tip
    pub const DEFAULT_TPS: u64 = 5;

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::Frankfurt => ASTRALANE_ENDPOINTS[0].to_string(),
            Region::LosAngeles => ASTRALANE_ENDPOINTS[1].to_string(),
            Region::Tokyo => ASTRALANE_ENDPOINTS[2].to_string(),
            Region::NewYork => ASTRALANE_ENDPOINTS[3].to_string(),
            Region::Amsterdam => ASTRALANE_ENDPOINTS[4].to_string(),
            _ => ASTRALANE_ENDPOINTS[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let endpoint = Self::get_endpoint().to_string();
        let auth_token = std::env::var("ASTRALANE_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        Astralane {
            endpoint,
            auth_token,
            http_client,
        }
    }

    fn get_tip_address(&self) -> Pubkey {
        *ASTRALANE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ASTRALANE_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for Astralane {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let req_json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                tx_base64,
                {
                    "encoding": "base64",
                    "skipPreflight": true,
                },
                { "mevProtect": true }
            ],
        });
        // println!("[astralane/send_tx] endpoint: {}", self.endpoint);
        // println!("[astralane/send_tx] api-key(header): {}", self.auth_token);
        // println!("[astralane/send_tx] request body: {}", req_json);
        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&req_json)
            .send()
            .await;
        // println!("[astralane/send_tx] res: {res:?}");
        let response = match res {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    // println!("[astralane/send_tx] response text error: {}", e);
                    return Err(format!("response text error: {}", e));
                }
            },
            Err(e) => {
                // println!("[astralane/send_tx] send error: {}", e);
                return Err(format!("send error: {}", e));
            }
        };
        // println!("[astralane/send_tx] response: {}", response);
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Astralane {
    async fn send_bundle(
        &self,
        txs: &[crate::platform_clients::SolTx],
    ) -> Result<Vec<Signature>, String> {
        // 将所有交易序列化并 base64 编码
        let mut encoded_txs = Vec::with_capacity(txs.len());
        let mut sigs: Vec<Signature> = Vec::with_capacity(txs.len());
        for tx in txs {
            let encode_tx = match bincode::serialize(tx) {
                Ok(bytes) => base64::prelude::BASE64_STANDARD.encode(&bytes),
                Err(e) => return Err(format!("bincode serialize error: {}", e)),
            };
            encoded_txs.push(encode_tx);
            sigs.push(tx.sig());
        }

        let request_body = match serde_json::to_string(&json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "sendBundle",
            "params": [
                encoded_txs,
                {
                    "encoding": "base64",
                    "mevProtect": true,
                    "revertProtection": false
                }
            ]
        })) {
            Ok(body) => body,
            Err(e) => return Err(format!("serde_json error: {}", e)),
        };

        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .body(request_body)
            .send()
            .await;

        
        let response = match res {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    // println!("[astralane/send_bundle] response text error: {}", e);
                    return Err(format!("response text error: {}", e));
                }
            },
            Err(e) => {
                log::error!("[astralane/send_bundle] send error: {:?}", e);
                return Err(format!("send error: {}", e));
            }
        };

        log::info!("astralane raw response: {:?}", response);

        // 尝试用结构体解析响应
        match serde_json::from_str::<AstralaneSendBundleResponse>(&response) {
            Ok(resp_obj) => {
                if let Some(result) = resp_obj.result {
                    log::info!("astralane bundle signatures: {:?}", result);
                    Ok(sigs)
                } else if let Some(err) = resp_obj.error {
                    Err(format!("astralane error: {}", err))
                } else {
                    Err(format!("astralane unknown response: {}", response))
                }
            }
            Err(e) => Err(format!(
                "astralane response parse error: {}, raw: {}",
                e, response
            )),
        }
    }
}

impl crate::platform_clients::BuildTx for Astralane {
    fn platform(&self) -> PlatformName {
        PlatformName::Astralane
    }
    fn get_tip_address(&self) -> Pubkey {
        *ASTRALANE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ASTRALANE_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for Astralane {
    fn build_bundle<'a>(
        &'a self,
        txs: &[crate::platform_clients::SolTx],
    ) -> crate::platform_clients::BundleEnvelope<'a, Astralane> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}

