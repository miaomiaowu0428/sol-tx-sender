use std::fmt;
impl fmt::Display for Stellium {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Stellium")
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
use crate::platform_clients::{PlatformName, Region};

// Stellium tip 地址
pub const STELLIUM_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("ste11JV3MLMM7x7EJUM2sXcJC1H7F4jBLnP9a9PG8PH"),
    pubkey!("ste11MWPjXCRfQryCshzi86SGhuXjF4Lv6xMXD2AoSt"),
    pubkey!("ste11p5x8tJ53H1NbNQsRBg1YNRd4GcVpxtDw8PBpmb"),
    pubkey!("ste11p7e2KLYou5bwtt35H7BM6uMdo4pvioGjJXKFcN"),
    pubkey!("ste11TMV68LMi1BguM4RQujtbNCZvf1sjsASpqgAvSX"),
];

// Stellium API 端点 - 支持多个区域
pub const STELLIUM_ENDPOINTS: &[&str] = &[
    "http://ewr1.flashrpc.com", // New York (EWR)
    "http://fra1.flashrpc.com", // Frankfurt
    "http://ams1.flashrpc.com", // Amsterdam
    "http://lhr1.flashrpc.com", // london
    "http://tyo1.flashrpc.com", // tokio
];

#[derive(Clone)]
pub struct Stellium {
    pub endpoint: String,
    pub api_key: String,
    pub http_client: Arc<Client>,
}

impl Stellium {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // Stellium 单笔交易最低 tip
    pub const DEFAULT_TPS: u64 = 10; // Stellium 默认 TPS

    /// 根据区域获取对应的端点
    pub fn get_endpoint_for_region(region: Region) -> String {
        match region {
            Region::NewYork => STELLIUM_ENDPOINTS[0].to_string(), // ewr1.flashrpc.com
            Region::Frankfurt => STELLIUM_ENDPOINTS[1].to_string(), // fra1.flashrpc.com
            Region::Amsterdam => STELLIUM_ENDPOINTS[2].to_string(), // ams1.flashrpc.com
            Region::London => STELLIUM_ENDPOINTS[3].to_string(),
            Region::Tokyo => STELLIUM_ENDPOINTS[4].to_string(),
            _ => STELLIUM_ENDPOINTS[1].to_string(),
        }
    }

    /// 获取当前区域的端点（用于 keep-alive）
    pub fn get_endpoint() -> String {
        Self::get_endpoint_for_region(*REGION)
    }

    pub fn new() -> Self {
        // 根据区域选择端点
        let region = *REGION;
        let endpoint = Self::get_endpoint_for_region(region);

        // 从环境变量获取 API key
        let api_key = std::env::var("STELLIUM_API_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();

        Stellium {
            endpoint,
            api_key,
            http_client,
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for Stellium {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        log_time!("stellium send: ", {
            // URL 格式：https://STELLIUM_ENDPOINT/$APIKEY
            let url = format!("{}/{}", self.endpoint, self.api_key);

            // 生成唯一的请求 ID（使用时间戳）
            let request_id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string();

            let res = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "method": "sendTransaction",
                    "params": [
                        tx_base64,
                        { "encoding": "base64" }
                    ]
                }))
                .send()
                .await;

            let response = match res {
                Ok(resp) => match resp.text().await {
                    Ok(text) => text,
                    Err(e) => return Err(format!("response text error: {}", e)),
                },
                Err(e) => {
                    log::error!("Stellium send error: {:?}", e);
                    return Err(format!("send error: {}", e));
                }
            };

            info!("Stellium response: {}", response);

            // 解析响应
            let parsed_response: serde_json::Value = serde_json::from_str(&response)
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            // 检查是否有错误
            if let Some(error) = parsed_response.get("error") {
                let error_code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                let error_message = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                return Err(format!(
                    "Stellium error (code {}): {}",
                    error_code, error_message
                ));
            }

            // 检查是否有结果
            if let Some(signature) = parsed_response.get("result").and_then(|r| r.as_str()) {
                info!("Stellium transaction signature: {}", signature);
                Ok(())
            } else {
                Err("Invalid response format from Stellium".to_string())
            }
        })
    }
}

impl crate::platform_clients::BuildTx for Stellium {
    fn get_tip_address(&self) -> Pubkey {
        *STELLIUM_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| STELLIUM_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn platform(&self) -> PlatformName {
        PlatformName::Stellium
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}
