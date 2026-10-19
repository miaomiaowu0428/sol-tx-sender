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
    "https://de1.0slot.trade", // Frankfurt
    "https://ams.0slot.trade", // Amsterdam
    "https://jp.0slot.trade",  // Tokyo
    "https://la.0slot.trade",  // LosAngeles
];

#[derive(Clone)]
pub struct ZeroSlot {
    pub endpoint: String,
    pub token: String,
    pub http_client: Arc<Client>,
}

impl ZeroSlot {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip

    /// 按区域返回入口 URL（纯函数，不依赖全局状态）。
    ///
    /// 0slot 没有机房的区域就近回退：
    /// - `Frankfurt2` → Frankfurt
    /// - `London` / `Dublin` / `Limburg` / `Lithuania` → Amsterdam
    /// - `Pittsburgh` → New York，`SaltLakeCity` → Los Angeles
    /// - `Unknown` → Frankfurt
    pub fn endpoint_for(region: Region) -> &'static str {
        match region {
            Region::NewYork | Region::Pittsburgh => ZEROSLOT_ENDPOINT[0],
            Region::Frankfurt | Region::Frankfurt2 => ZEROSLOT_ENDPOINT[1],
            Region::Amsterdam
            | Region::London
            | Region::Dublin
            | Region::Limburg
            | Region::Lithuania => ZEROSLOT_ENDPOINT[2],
            Region::Tokyo => ZEROSLOT_ENDPOINT[3],
            Region::LosAngeles | Region::SaltLakeCity => ZEROSLOT_ENDPOINT[4],
            Region::Singapore => ZEROSLOT_ENDPOINT[3],
            Region::Unknown => ZEROSLOT_ENDPOINT[1],
        }
    }

    /// 兼容旧调用：从 `REGION` 全局 static 读取地区。
    ///
    /// 注意：不再对未知地区返回空串（旧实现 `get_endpoint()` 返回 `""`，而
    /// `new()` 却悄悄指向 New York，两条路径不一致）。现在统一走就近回退。
    pub fn get_endpoint() -> String {
        Self::endpoint_for(*REGION).to_string()
    }

    pub fn new() -> Self {
        let endpoint = Self::get_endpoint();
        let token = std::env::var("ZEROSLOT_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        ZeroSlot {
            endpoint,
            token,
            http_client,
        }
    }

    /// 显式构造：调用方负责提供 key 和 region，不读取任何环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        let endpoint = Self::endpoint_for(region).to_string();
        ZeroSlot {
            endpoint,
            token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for ZeroSlot {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("0slot send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
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
        })
    }
}

impl crate::platform_clients::BuildTx for ZeroSlot {
    fn get_tip_address(&self) -> Pubkey {
        *ZEROSLOT_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ZEROSLOT_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Zeroslot
    }
    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    fn tip_recvs(&self) -> Vec<Pubkey> {
        ZEROSLOT_TIP_ACCOUNTS.to_vec()
    }

    // 使用默认实现，无需重写 build_tx
}

impl std::fmt::Display for ZeroSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ZeroSlot")
    }
}
