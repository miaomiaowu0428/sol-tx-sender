use std::env;
use base64::Engine;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::signature::Signature;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region, SendTxEncoded, SolTx};
pub const FLASH_BLOCK_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("FLaShB3iXXTWE1vu9wQsChUKq3HFtpMAhb8kAh1pf1wi"),
    pubkey!("FLashhsorBmM9dLpuq6qATawcpqk1Y2aqaZfkd48iT3W"),
    pubkey!("FLaSHJNm5dWYzEgnHJWWJP5ccu128Mu61NJLxUf7mUXU"),
    pubkey!("FLaSHR4Vv7sttd6TyDF4yR1bJyAxRwWKbohDytEMu3wL"),
    pubkey!("FLASHRzANfcAKDuQ3RXv9hbkBy4WVEKDzoAgxJ56DiE4"),
    pubkey!("FLasHstqx11M8W56zrSEqkCyhMCCpr6ze6Mjdvqope5s"),
    pubkey!("FLAShWTjcweNT4NSotpjpxAkwxUr2we3eXQGhpTVzRwy"),
    pubkey!("FLasHXTqrbNvpWFB6grN47HGZfK6pze9HLNTgbukfPSk"),
    pubkey!("FLAshyAyBcKb39KPxSzXcepiS8iDYUhDGwJcJDPX4g2B"),
    pubkey!("FLAsHZTRcf3Dy1APaz6j74ebdMC6Xx4g6i9YxjyrDybR"),
];

// 美国纽约: http://ny.flashblock.trade
// 美国盐湖城: http://slc.flashblock.trade
// 荷兰阿姆斯特丹: http://ams.flashblock.trade
// 德国法兰克福: http://fra.flashblock.trade
// 新加坡: http://singapore.flashblock.trade
// 英国伦敦：http://london.flashblock.trade
pub const FLASH_BLOCK_ENDPOINTS: &[&str] = &[
    "http://ny.flashblock.trade",
    "http://slc.flashblock.trade",
    "http://ams.flashblock.trade",
    "http://fra.flashblock.trade",
    "http://singapore.flashblock.trade",
    "http://london.flashblock.trade",
];

#[derive(Clone)]
pub struct FlashBlock {
    pub endpoint: String,
    pub http_client: Arc<Client>,
    pub auth_token: String,
}

#[derive(Debug, serde::Deserialize)]
struct FlashBlockSendBundleResponse {
    pub jsonrpc: Option<String>,
    pub result: Option<String>,
    pub error: Option<serde_json::Value>,
    pub id: Option<u64>,
}
use std::fmt;
impl fmt::Display for FlashBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FlashBlock<{}>", self.endpoint)
    }
}

impl FlashBlock {
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_001_000_000; // 单笔交易最低 tip
    pub const DEFAULT_TPS:u64 = 10;

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => FLASH_BLOCK_ENDPOINTS[0].to_string(),
            Region::SaltLakeCity => FLASH_BLOCK_ENDPOINTS[1].to_string(),
            Region::Amsterdam => FLASH_BLOCK_ENDPOINTS[2].to_string(),
            Region::Frankfurt => FLASH_BLOCK_ENDPOINTS[3].to_string(),
            Region::Singapore => FLASH_BLOCK_ENDPOINTS[4].to_string(),
            Region::London => FLASH_BLOCK_ENDPOINTS[5].to_string(),
            _ => FLASH_BLOCK_ENDPOINTS[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => FLASH_BLOCK_ENDPOINTS[0].to_string(),
            Region::SaltLakeCity => FLASH_BLOCK_ENDPOINTS[1].to_string(),
            Region::Amsterdam => FLASH_BLOCK_ENDPOINTS[2].to_string(),
            Region::Frankfurt => FLASH_BLOCK_ENDPOINTS[3].to_string(),
            Region::Singapore => FLASH_BLOCK_ENDPOINTS[4].to_string(),
            Region::London => FLASH_BLOCK_ENDPOINTS[5].to_string(),
            _ => FLASH_BLOCK_ENDPOINTS[0].to_string(),
        };
        let http_client = HTTP_CLIENT.clone();
        let auth_token = env::var("FLASHBLOCK_KEY").unwrap_or_default();
        FlashBlock {
            endpoint,
            http_client,
            auth_token
        }
    }

    fn get_tip_address() -> Pubkey {
        *FLASH_BLOCK_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| FLASH_BLOCK_TIP_ACCOUNTS.first())
            .unwrap()
    }
}


#[async_trait::async_trait]
impl SendTxEncoded for FlashBlock {

    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let request_body = match serde_json::to_string(&json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "sendBundle",
            "params": [
                [tx_base64],
                { "encoding": "base64" }
            ]
        })) {
            Ok(body) => body,
            Err(e) => return Err(format!("serde_json error: {}", e)),
        };
        let url = format!("{}/", self.endpoint);
    
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", self.auth_token.clone())
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
        log::info!("flashblock raw response: {:?}", response);
        // 尝试用结构体解析响应
        match serde_json::from_str::<FlashBlockSendBundleResponse>(&response) {
            Ok(resp_obj) => {
                if let Some(result) = resp_obj.result {
                    log::info!("flashblock bundle id: {}", result);
                    Ok(())
                } else if let Some(err) = resp_obj.error {
                    Err(format!("flashblock error: {}", err))
                } else {
                    Err(format!("flashblock unknown response: {}", response))
                }
            }
            Err(e) => Err(format!(
                "flashblock response parse error: {}, raw: {}",
                e, response
            )),
        }
    }
}





#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for FlashBlock {
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
        let url = format!("{}/", self.endpoint);
    
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", self.auth_token.clone())
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
        log::info!("flashblock raw response: {:?}", response);
        // 尝试用结构体解析响应
        match serde_json::from_str::<FlashBlockSendBundleResponse>(&response) {
            Ok(resp_obj) => {
                if let Some(result) = resp_obj.result {
                    log::info!("flashblock bundle id: {}", result);
                    Ok(sigs)
                } else if let Some(err) = resp_obj.error {
                    Err(format!("flashblock error: {}", err))
                } else {
                    Err(format!("flashblock unknown response: {}", response))
                }
            }
            Err(e) => Err(format!(
                "flashblock response parse error: {}, raw: {}",
                e, response
            )),
        }
    }
}

impl crate::platform_clients::BuildTx for FlashBlock {
    fn get_tip_address(&self) -> Pubkey {
        Self::get_tip_address()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::FlashBlock
    }
    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for FlashBlock {
    fn build_bundle<'a>(
        &'a self,
        txs: &[SolTx],
    ) -> crate::platform_clients::BundleEnvelope<'a, FlashBlock> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}
