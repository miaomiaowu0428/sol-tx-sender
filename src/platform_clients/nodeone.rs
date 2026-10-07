use std::fmt;
impl fmt::Display for NodeOne {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeOne")
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

/// Node1 tip 地址（官方 24 个，随机选取以分散负载）。
/// 是否使用以 `tip_recvs()` 为准，`get_tip_address()` 随机挑一个。
pub const NODEONE_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("Node1DWLLHtpKXQUe1FvLgANTgx9qknWDBMCgB9Dbc8"),
    pubkey!("Node1DXv6DFbzJKCfJF3wcsLg1KYm3AHAMAq7jFTZ44"),
    pubkey!("Node1GcMxYVrr5FABQxbvWn7AY9UjeurZUu2BCcUEJb"),
    pubkey!("Node1H5SG2b7Qa9KRrDGtPfffQ8JsdYePur4RcJViqq"),
    pubkey!("Node1Jai3s5yU5kXZnrECbupzhdVmPh3p1MYRnwvruH"),
    pubkey!("Node1JvDo7mKGh3hqXVXTeohksejQuh385tnoEsYsxS"),
    pubkey!("Node1JwcWbikWff65JdVD16vTor2TYSaFKvHWQhn2ZK"),
    pubkey!("Node1KGHLPrvZbAFqaauTFFnMHYU6MshJWTRBrUQfYg"),
    pubkey!("Node1Vm76GupuHSvHZJr4Unojz1EBGAQ1jjaWzLhLzX"),
    pubkey!("Node1bqVym2A6wMcXaaoEns91GsY78A3vZFNWahxkFY"),
    pubkey!("Node1cEZ1Dgmi188mvm6fsTW5vw4nGRArcuXcssBq7i"),
    pubkey!("Node1cecy1NeGSLburSQhZp75TZN6o58aTaRJWXTgnX"),
    pubkey!("Node1dkDBeE3ZYyCE1YXZvxs6nK91fFmt1NxasZsCf7"),
    pubkey!("Node1nF4fmy7Za7LrTLQEatfy77KW5xZVmFsg4PGwWf"),
    pubkey!("Node1o1bivr8Xeb2qyLn4LDWdNVnFYNS2CQwx2zxHDq"),
    pubkey!("Node1oLXzJ9A1Q5RxvEXfbTj6JDU5BaZhBhJRXEgKpg"),
    pubkey!("Node1oe6BgwdxJXdkoXq24XZo7oMS7UQExonUhAcfaq"),
    pubkey!("Node1ovdUHugTxbCUydcEbXRG71BFWM1J3Zp7WPiqGj"),
    pubkey!("Node1pyyf5SKsNVAVAB7pS1J6KZjesbRuT3MHAn1M5Q"),
    pubkey!("Node1qdrBBhNu73jnvxZNJPYXtusdeCapbP8A3C8PLK"),
    pubkey!("Node1smSyGSEoPWrANwbhfeRBSkHfq5PF8vsE3r4Be4"),
    pubkey!("Node1vmD7f7GYF5ghRS27Rp4fNBdtCGEcSzkiXjdpnP"),
    pubkey!("Node1yVhvYnU8fqgiHULQzkbJnfgmUCxesPYJBz4STe"),
    pubkey!("Node1zoA9spwBsUQS8dZNmCjEoyyJKFybgbTKCs6p3K"),
];

// Node1 接入点（官方 8 个地区，HTTP/HTTPS 均可，建议长连接 Keep-Alive）
pub const NODEONE_ENDPOINT: &[&str] = &[
    "https://ny.node1.me",   // New York
    "https://ams.node1.me",  // Amsterdam
    "https://fra.node1.me",  // Frankfurt
    "https://lon.node1.me",  // London
    "https://tk.node1.me",   // Tokyo
    "https://sgp.node1.me",  // Singapore
    "https://fra2.node1.me", // Frankfurt 2
    "https://dub.node1.me",  // Dublin
];

#[derive(Clone)]
pub struct NodeOne {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl NodeOne {
    pub const MIN_TIP_AMOUNT_TX: u64 = 100_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 2_000_000; // 批量交易最低 tip

    /// 按区域选择 Node1 接入点；没有对应机房时回退到 New York。
    fn endpoint_for(region: Region) -> String {
        let idx = match region {
            Region::NewYork => 0,
            Region::Amsterdam => 1,
            Region::Frankfurt => 2,
            Region::London => 3,
            Region::Tokyo => 4,
            Region::Singapore => 5,
            Region::Frankfurt2 => 6,
            Region::Dublin => 7,
            _ => 0,
        };
        NODEONE_ENDPOINT[idx].to_string()
    }

    pub fn get_endpoint() -> String {
        Self::endpoint_for(*REGION)
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = Self::endpoint_for(region);
        let auth_token = std::env::var("NODEONE_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        NodeOne {
            endpoint,
            auth_token,
            http_client,
        }
    }

    /// 显式构造：调用方负责提供 key 和 region，不读取任何环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        let endpoint = Self::endpoint_for(region);
        NodeOne {
            endpoint,
            auth_token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for NodeOne {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("node1 send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
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
        })
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
    fn tip_recvs(&self) -> Vec<Pubkey> {
        NODEONE_TIP_ACCOUNTS.to_vec()
    }
    // 使用默认实现，无需重写 build_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 24 个 tip 地址，全部唯一且非空（防止复制粘贴漏改/重复）。
    #[test]
    fn test_tip_accounts_unique_and_complete() {
        assert_eq!(NODEONE_TIP_ACCOUNTS.len(), 24);
        let mut uniq = NODEONE_TIP_ACCOUNTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 24, "NODEONE_TIP_ACCOUNTS 存在重复地址");
    }

    /// 8 个接入点按区域一一对应。
    #[test]
    fn test_endpoint_per_region() {
        assert_eq!(
            NodeOne::endpoint_for(Region::NewYork),
            "https://ny.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Amsterdam),
            "https://ams.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Frankfurt),
            "https://fra.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::London),
            "https://lon.node1.me"
        );
        assert_eq!(NodeOne::endpoint_for(Region::Tokyo), "https://tk.node1.me");
        assert_eq!(
            NodeOne::endpoint_for(Region::Singapore),
            "https://sgp.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Frankfurt2),
            "https://fra2.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Dublin),
            "https://dub.node1.me"
        );
        // 无 Node1 机房的区域回退到 New York
        assert_eq!(
            NodeOne::endpoint_for(Region::LosAngeles),
            "https://ny.node1.me"
        );
    }
}
