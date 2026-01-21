use std::fmt;
impl fmt::Display for NextBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NextBlock")
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

// NextBlock MEV 保护和 tip 地址
pub const NEXTBLOCK_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("nextBLoCkPMgmG8ZgJtABeScP35qLa2AMCNKntAP7Xc"),
    pubkey!("NEXTbLoCkB51HpLBLojQfpyVAMorm3zzKg7w9NFdqid"),
    pubkey!("nEXTBLockYgngeRmRrjDV31mGSekVPqZoMGhQEZtPVG"),
    pubkey!("neXtBLock1LeC67jYd1QdAa32kbVeubsfPNTJC1V5At"),
    pubkey!("NexTBLockJYZ7QD7p2byrUa6df8ndV2WSd8GkbWqfbb"),
    pubkey!("NeXTBLoCKs9F1y5PJS9CKrFNNLU1keHW71rfh7KgA1X"),
    pubkey!("NextbLoCkVtMGcV47JzewQdvBpLqT9TxQFozQkN98pE"),
    pubkey!("NexTbLoCkWykbLuB1NkjXgFWkX9oAtcoagQegygXXA2"),
];

// NextBlock API 端点 - 支持多个区域
pub const NEXTBLOCK_ENDPOINTS: &[&str] = &[
    "https://frankfurt.nextblock.io", // Frankfurt 法兰克福
    "https://amsterdam.nextblock.io", // Amsterdam 阿姆斯特丹
    "https://london.nextblock.io",    // London 伦敦
    "https://singapore.nextblock.io", // Singapore 新加坡
    "https://tokyo.nextblock.io",     // Tokyo 东京
    "https://ny.nextblock.io",        // New York 纽约
    "https://slc.nextblock.io",       // Salt Lake City 盐湖城
];

// 默认使用法兰克福端点
pub const NEXTBLOCK_ENDPOINT: &str = "https://frankfurt.nextblock.io"; // 法兰克福地区端点

#[derive(Clone)]
pub struct NextBlock {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl NextBlock {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // NextBlock 单笔交易最低 tip（需要根据实际情况调整）
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // NextBlock 批量交易最低 tip
    pub const DEFAULT_TPS: u64 = 5; // NextBlock 默认 TPS

    /// 根据区域获取对应的端点
    pub fn get_endpoint_for_region(region: Region) -> String {
        match region {
            Region::Frankfurt => NEXTBLOCK_ENDPOINTS[0].to_string(), // frankfurt.nextblock.io
            Region::Amsterdam => NEXTBLOCK_ENDPOINTS[1].to_string(), // amsterdam.nextblock.io
            Region::London => NEXTBLOCK_ENDPOINTS[2].to_string(),    // london.nextblock.io
            Region::Singapore => NEXTBLOCK_ENDPOINTS[3].to_string(), // singapore.nextblock.io
            Region::Tokyo => NEXTBLOCK_ENDPOINTS[4].to_string(),     // tokyo.nextblock.io
            Region::NewYork => NEXTBLOCK_ENDPOINTS[5].to_string(),   // ny.nextblock.io
            Region::SaltLakeCity => NEXTBLOCK_ENDPOINTS[6].to_string(), // slc.nextblock.io
            _ => NEXTBLOCK_ENDPOINT.to_string(),                     // 默认法兰克福
        }
    }

    pub fn get_endpoint() -> String {
        Self::get_endpoint_for_region(*REGION)
    }

    pub fn new() -> Self {
        // 根据区域选择端点
        let region = *REGION;
        let endpoint = Self::get_endpoint_for_region(region);

        // 从环境变量获取 token，可能需要 Bearer 前缀
        let auth_token = std::env::var("NEXTBLOCK_TOKEN")
            // .or_else(|_| std::env::var("AUTH_HEADER"))
            .unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        NextBlock {
            endpoint,
            auth_token,
            http_client,
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for NextBlock {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        log_time!("next block send: ", {
            let url = format!("{}/api/v2/submit", self.endpoint);

            let res = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", &self.auth_token)
                .json(&json! ({
                    "transaction": {
                        "content": tx_base64
                    },
                    "frontRunningProtection": true
                }))
                .send()
                .await;

            let response = match res {
                Ok(resp) => match resp.text().await {
                    Ok(text) => text,
                    Err(e) => return Err(format!("response text error: {}", e)),
                },
                Err(e) => {
                    log::error!("NextBlock send error: {:?}", e);
                    return Err(format!("send error: {}", e));
                }
            };

            info!("NextBlock response: {}", response);
            Ok(())
        })
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for NextBlock {
    async fn send_bundle(
        &self,
        txs: &[crate::platform_clients::SolTx],
    ) -> Result<Vec<solana_sdk::signature::Signature>, String> {
        // NextBlock 要求 2-4 笔交易
        if txs.len() < 2 || txs.len() > 4 {
            return Err(format!(
                "NextBlock bundle requires 2-4 transactions, got {}",
                txs.len()
            ));
        }

        let url = format!("{}/api/v2/submit-batch", self.endpoint);

        // 构建 entries 数组
        let mut entries = Vec::new();
        for tx in txs {
            let tx_base64 = tx
                .to_base64()
                .map_err(|e| format!("Failed to serialize transaction: {}", e))?;
            entries.push(json!({
                "transaction": {
                    "content": tx_base64
                }
            }));
        }

        let request_body = json!({
            "entries": entries
        });

        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", &self.auth_token)
            .json(&request_body)
            .send()
            .await;

        let response = match res {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => return Err(format!("response text error: {}", e)),
            },
            Err(e) => {
                log::error!("NextBlock bundle send error: {:?}", e);
                return Err(format!("bundle send error: {}", e));
            }
        };

        info!("NextBlock bundle response: {}", response);

        // 解析响应
        let parsed_response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(signature_str) = parsed_response.get("signature").and_then(|s| s.as_str()) {
            // NextBlock 只返回一个签名，我们需要为每个交易返回相同的签名
            let signature = signature_str
                .parse::<solana_sdk::signature::Signature>()
                .map_err(|e| format!("Failed to parse signature: {}", e))?;

            // 返回与交易数量相同的签名数组
            Ok(vec![signature; txs.len()])
        } else if let Some(error_msg) = parsed_response.get("message").and_then(|m| m.as_str()) {
            Err(format!("NextBlock error: {}", error_msg))
        } else {
            Err("Invalid response format from NextBlock".to_string())
        }
    }
}

impl crate::platform_clients::BuildTx for NextBlock {
    fn get_tip_address(&self) -> Pubkey {
        *NEXTBLOCK_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| NEXTBLOCK_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn tip_recvs(&self) -> Vec<Pubkey> {
        NEXTBLOCK_TIP_ACCOUNTS.to_vec()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Nextblock
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}
