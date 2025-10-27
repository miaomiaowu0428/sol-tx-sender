/// Jito sendBundle 响应结构体
#[derive(Debug, serde::Deserialize)]
struct JitoSendBundleResponse {
    pub _jsonrpc: Option<String>,
    pub result: Option<String>,
    pub error: Option<serde_json::Value>,
    pub _id: Option<u64>,
}
use std::fmt;
impl fmt::Display for Jito {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Jito")
    }
}
use base64::Engine;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::signature::Signature;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region, SolTx};
pub const JITO_TIP_ACCOUNTS: &[Pubkey] = &[
    // pubkey!("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"),
    pubkey!("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe"),
    pubkey!("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY"),
    pubkey!("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49"),
    pubkey!("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh"),
    pubkey!("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt"),
    pubkey!("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL"),
    pubkey!("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"),
];

pub const JITO_ENDPOINTS: &[&str] = &[
    "https://ny.mainnet.block-engine.jito.wtf", // NY
    "https://frankfurt.mainnet.block-engine.jito.wtf",
    "https://amsterdam.mainnet.block-engine.jito.wtf",
    "https://london.mainnet.block-engine.jito.wtf", // london
    "https://slc.mainnet.block-engine.jito.wtf",    //
    "https://tokyo.mainnet.block-engine.jito.wtf",
    "https://singapore.mainnet.block-engine.jito.wtf",
];

#[derive(Clone)]
pub struct Jito {
    pub endpoint: String,
    pub http_client: Arc<Client>,
}

impl Jito {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 10_000; // 批量交易最低 tip
    pub const DEFAULT_TPS: u64 = 1;

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => JITO_ENDPOINTS[0].to_string(),
            Region::Frankfurt => JITO_ENDPOINTS[1].to_string(),
            Region::Amsterdam => JITO_ENDPOINTS[2].to_string(),
            Region::London => JITO_ENDPOINTS[3].to_string(),
            Region::SaltLakeCity => JITO_ENDPOINTS[4].to_string(),
            Region::Tokyo => JITO_ENDPOINTS[5].to_string(),
            Region::Singapore => JITO_ENDPOINTS[6].to_string(),
            _ => JITO_ENDPOINTS[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => JITO_ENDPOINTS[0].to_string(),
            Region::Frankfurt => JITO_ENDPOINTS[1].to_string(),
            Region::Amsterdam => JITO_ENDPOINTS[2].to_string(),
            Region::London => JITO_ENDPOINTS[3].to_string(),
            Region::SaltLakeCity => JITO_ENDPOINTS[4].to_string(),
            Region::Tokyo => JITO_ENDPOINTS[5].to_string(),
            Region::Singapore => JITO_ENDPOINTS[6].to_string(),
            _ => JITO_ENDPOINTS[0].to_string(),
        };
        let http_client = HTTP_CLIENT.clone();
        Jito {
            endpoint,
            http_client,
        }
    }

    fn get_tip_address() -> Pubkey {
        *JITO_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| JITO_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for Jito {
    /// 直接接收 base64 编码后的交易数据并发送
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let request_body = match serde_json::to_string(&json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "sendTransaction",
            "params": [
                tx_base64,
                { "encoding": "base64" }
            ]
        })) {
            Ok(body) => body,
            Err(e) => return Err(format!("serde_json error: {}", e)),
        };
        let url = format!("{}/api/v1/bundles", self.endpoint);
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(request_body)
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
        log::info!("jito response: {:?}", response);
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Jito {
    async fn send_bundle(&self, txs: &[SolTx]) -> Result<Vec<Signature>, String> {
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
                { "encoding": "base64" }
            ]
        })) {
            Ok(body) => body,
            Err(e) => return Err(format!("serde_json error: {}", e)),
        };
        let url = format!("{}/api/v1/bundles", self.endpoint);
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(request_body)
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
        log::info!("jito raw response: {:?}", response);
        // 尝试用结构体解析响应
        match serde_json::from_str::<JitoSendBundleResponse>(&response) {
            Ok(resp_obj) => {
                if let Some(result) = resp_obj.result {
                    log::info!("jito bundle id: {}", result);
                    Ok(sigs)
                } else if let Some(err) = resp_obj.error {
                    Err(format!("jito error: {}", err))
                } else {
                    Err(format!("jito unknown response: {}", response))
                }
            }
            Err(e) => Err(format!(
                "jito response parse error: {}, raw: {}",
                e, response
            )),
        }
    }
}

impl crate::platform_clients::BuildTx for Jito {
    fn get_tip_address(&self) -> Pubkey {
        Self::get_tip_address()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Jito
    }
    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for Jito {
    fn build_bundle<'a>(
        &'a self,
        txs: &[SolTx],
    ) -> crate::platform_clients::BundleEnvelope<'a, Jito> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}
