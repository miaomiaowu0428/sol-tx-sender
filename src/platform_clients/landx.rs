//! **LandX** transaction sender（SWQoS）平台客户端。
//!
//! 官方文档要点（详见 `docs/landx.md`）：
//!
//! - Base URL 同时支持 HTTP / UDP；**HTTP 走本文件，UDP 走 `landx_udp.rs`**。HTTP 下 API key 必须
//!   放在 **path** 里（`http://<host>/<api_key>`），不支持 query-string / header 鉴权。
//! - 每笔交易必须包含一笔打给指定 tip 地址的 **SystemProgram transfer**。
//! - 默认最低 tip：**0.001 SOL = 1,000,000 lamports**。
//! - JSON-RPC 只用 **base64** 编码。
//! - 复用 HTTP keep-alive 连接（走 [`crate::constants::HTTP_CLIENT`] 共享连接池）。
//!
//! # 区域端点
//!
//! | Region | Host | 提交方式 |
//! |---|---|---|
//! | Amsterdam | `ams1.landx.dev` | HTTP / UDP |
//! | Amsterdam2 | `ams2.landx.dev` | HTTP / UDP |
//! | Frankfurt1 | `fra1.landx.dev` | HTTP / UDP |
//! | Frankfurt2 | `fra2.landx.dev` | 仅 UDP |
//! | Frankfurt3 | `fra3.landx.dev` | 仅 UDP |
//! | New York | `ny1.landx.dev` | HTTP / UDP |
//! | Tokyo | `tyo1.landx.dev` | HTTP / UDP |
//! | London | `lon1.landx.dev` | HTTP / UDP |
//! | Singapore | `sgp1.landx.dev` | HTTP / UDP |
//! | Salt Lake City | `slc1.landx.dev` | 仅 UDP |
//! | Dublin | `dub1.landx.dev` | 仅 UDP |
//!
//! ⚠️ `fra2` / `fra3` / `slc1` / `dub1` **只有 UDP**，不走 HTTP，因此
//! [`LandX::endpoint_for_region`] 会给这些区域回退到最近的 HTTP 可用机房
//! （Frankfurt2→fra1、SaltLakeCity→ny1、Dublin→lon1）。要打这四个机房请用
//! [`crate::platform_clients::landx_udp::LandXUdp`]（它按区域直连 fra2/slc1/dub1）。
//!
//! # 鉴权 key
//!
//! `new()` 从环境变量 **`LANDX_API_KEY`** 读取；[`LandX::init_with`] 显式传入。
//!
//! # 关于 bundle：**未实现**
//!
//! 文档只描述单笔交易的提交与 tip 规则，没有提及 `sendBundle`。因此只实现
//! [`SendTx`] / [`BuildTx`]，等官方确认 bundle 协议后再补。

use std::fmt;
use std::sync::Arc;

use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use utils::log_time;

use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{BuildTx, PlatformName, Region, SendTx, TxExt};

/// 官方指定的 10 个 tip 地址。
///
/// 地址同时用于：交易内 tip 收款、[`BuildTx::tip_recvs`]。
pub const LANDX_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("LandX1EMgYtrfi4jVJ7ryby4mDpZcef6sVH9b9U1cog"),
    pubkey!("LandX2gHE7y6RejuvoLJK4hJiKkrAwcrJJ34u19Gmo1"),
    pubkey!("LandX3uK1cSUF7hbefKo3oL1j4FhvhxitiMRKut99Z6"),
    pubkey!("LandX4wXzvDyWfq1XXn4StRjwPjzqb7je2kqDGLoT9L"),
    pubkey!("LandX5T4jfRASrcn76owxHM4q9bduNGj24ev4Ufc5fP"),
    pubkey!("LandX6RsjjnfaxfYAFTZX1TW9Cgfe9HFStj9nRDd5SK"),
    pubkey!("LandX7LsdzXDAbbyBjAq5FxLTnGQaU3LojGQMLZCvkt"),
    pubkey!("LandX8Gv3UkhjmgKdSEg4F7yG7Z2tiqhNDYDtxazydN"),
    pubkey!("LandX9fhNN5S7fuBgekKBYeYvK1Sy9hbGvDmHsBGYFh"),
    pubkey!("LandXXxDXaSS8MjrqG9nri51htrfM6V4R3zXDVCeV8R"),
];

/// **HTTP 可用**的区域 base URL。
///
/// 仅 UDP 的机房（fra2 / fra3 / slc1 / dub1）不在此列，见模块文档。
pub const LANDX_HTTP_HOSTS: &[&str] = &[
    "http://ams1.landx.dev", // Amsterdam (TeraSwitch)
    "http://ams2.landx.dev", // Amsterdam2 (Cherry Servers)
    "http://fra1.landx.dev", // Frankfurt1 (TeraSwitch)
    "http://ny1.landx.dev",  // New York (TeraSwitch)
    "http://tyo1.landx.dev", // Tokyo (TeraSwitch)
    "http://lon1.landx.dev", // London (TeraSwitch)
    "http://sgp1.landx.dev", // Singapore (TeraSwitch)
];

/// 仅 UDP、无法走 HTTP 的 host（由 `landx_udp::LandXUdp` 使用）。
pub const LANDX_UDP_ONLY_HOSTS: &[&str] = &["fra2.landx.dev", "fra3.landx.dev", "slc1.landx.dev", "dub1.landx.dev"];

/// LandX HTTP 交易发送客户端。
#[derive(Clone)]
pub struct LandX {
    /// 区域 base URL，例如 `http://fra1.landx.dev`（**不含 key**）。
    pub endpoint: String,
    /// API key；HTTP 提交时拼进 path。
    pub api_key: String,
    pub http_client: Arc<Client>,
}

impl LandX {
    /// 单笔交易最低 tip：**0.001 SOL**。
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000;

    /// 每个区域对应的 HTTP base URL；无机房或仅 UDP 的区域就近回退。
    pub fn endpoint_for_region(region: Region) -> &'static str {
        match region {
            Region::Amsterdam => LANDX_HTTP_HOSTS[0], // ams1
            Region::Frankfurt => LANDX_HTTP_HOSTS[2], // fra1
            // fra2/fra3 仅 UDP → 回退 fra1
            Region::Frankfurt2 => LANDX_HTTP_HOSTS[2],
            Region::NewYork | Region::LosAngeles | Region::Pittsburgh => LANDX_HTTP_HOSTS[3], // ny1
            Region::Tokyo => LANDX_HTTP_HOSTS[4],                                             // tyo1
            Region::London | Region::Dublin => LANDX_HTTP_HOSTS[5],                           // lon1（dub1 仅 UDP）
            Region::Singapore => LANDX_HTTP_HOSTS[6],                                         // sgp1
            // slc1 仅 UDP → 回退 ny1
            Region::SaltLakeCity => LANDX_HTTP_HOSTS[3],
            Region::Limburg | Region::Lithuania | Region::Unknown => LANDX_HTTP_HOSTS[2], // fra1
        }
    }

    /// 当前区域 base URL。
    pub fn get_endpoint() -> String {
        Self::endpoint_for_region(*REGION).to_string()
    }

    /// 当前区域的 **HTTP 健康检查** URL：`http://<host>/ping`（成功返回 `ok`）。
    pub fn get_ping_endpoint() -> String {
        format!("{}/ping", Self::endpoint_for_region(*REGION))
    }

    /// 默认构造：`LANDX_API_KEY` + `REGION`。
    pub fn new() -> Self {
        Self {
            endpoint: Self::endpoint_for_region(*REGION).to_string(),
            api_key: std::env::var("LANDX_API_KEY").unwrap_or_default(),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 显式构造：调用方提供 key 与 region，不读环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        Self {
            endpoint: Self::endpoint_for_region(region).to_string(),
            api_key: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// HTTP 提交 URL：`<base>/<api_key>` —— key 必须在 path 里。
    pub fn send_url(&self) -> Result<String, String> {
        if self.api_key.is_empty() {
            return Err("LANDX_API_KEY is empty".into());
        }
        Ok(format!("{}/{}", self.endpoint.trim_end_matches('/'), self.api_key))
    }

    /// tip 地址：10 个官方地址里随机取一个。
    pub fn get_tip_address(&self) -> Pubkey {
        *LANDX_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| LANDX_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

impl Default for LandX {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON-RPC 成功/失败响应。
#[derive(Debug, serde::Deserialize)]
struct LandXRpcResponse {
    result: Option<String>,
    error: Option<LandXRpcError>,
}

/// JSON-RPC 错误体。
#[derive(Debug, serde::Deserialize)]
struct LandXRpcError {
    code: i64,
    message: String,
}

#[async_trait::async_trait]
impl SendTx for LandX {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("landx send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
            let url = self.send_url()?;

            let res = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&json!({
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

            let resp = match res {
                Ok(resp) => resp,
                Err(e) => {
                    log::error!("landx send error: {:?}", e);
                    return Err(format!("landx send error: {e}"));
                }
            };

            let status = resp.status();
            // 429 时服务端可能带 Retry-After（秒）
            let retry_after = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let text = resp.text().await.map_err(|e| format!("landx response text error: {e}"))?;

            if status.as_u16() == 429 {
                return Err(format!(
                    "landx rate limited (HTTP 429), retry after {}s: {text}",
                    retry_after.as_deref().unwrap_or("1")
                ));
            }
            if !status.is_success() {
                return Err(format!("landx HTTP {status}: {text}"));
            }

            match serde_json::from_str::<LandXRpcResponse>(&text) {
                Ok(parsed) => {
                    if let Some(err) = parsed.error {
                        return Err(format!("landx error {}: {}", err.code, err.message));
                    }
                    match parsed.result {
                        Some(sig) => {
                            info!("landx: submitted {sig}");
                            Ok(())
                        }
                        None => Err(format!("landx unexpected response: {text}")),
                    }
                }
                Err(e) => Err(format!("landx response parse error: {e}, raw: {text}")),
            }
        })
    }
}

impl BuildTx for LandX {
    fn get_tip_address(&self) -> Pubkey {
        self.get_tip_address()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    fn platform(&self) -> PlatformName {
        PlatformName::LandX
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        LANDX_TIP_ACCOUNTS.to_vec()
    }

    // uses_tip_transfer() 保持默认 true —— 文档要求每笔交易都必须带 tip transfer。
    // uses_cu_price() 保持默认 true —— 同时带 priority fee 争取调度优先。
}

impl fmt::Display for LandX {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LandX")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_clients::BuildTx;

    /// 官方 10 个 tip 地址，数量正确且无重复。
    #[test]
    fn test_tip_accounts_unique_and_complete() {
        assert_eq!(LANDX_TIP_ACCOUNTS.len(), 10);
        let mut uniq = LANDX_TIP_ACCOUNTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 10, "LANDX_TIP_ACCOUNTS 存在重复地址");
    }

    /// 区域 → host 映射；仅 UDP / 无机房的区域回退到 HTTP 可用机房。
    #[test]
    fn test_endpoint_per_region() {
        assert_eq!(LandX::endpoint_for_region(Region::Amsterdam), "http://ams1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::Frankfurt), "http://fra1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::NewYork), "http://ny1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::Tokyo), "http://tyo1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::London), "http://lon1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::Singapore), "http://sgp1.landx.dev");
        // 仅 UDP → 回退 HTTP 机房
        assert_eq!(LandX::endpoint_for_region(Region::Frankfurt2), "http://fra1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::SaltLakeCity), "http://ny1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::Dublin), "http://lon1.landx.dev");
        // 无 LandX 机房的区域就近回退
        assert_eq!(LandX::endpoint_for_region(Region::LosAngeles), "http://ny1.landx.dev");
        assert_eq!(LandX::endpoint_for_region(Region::Lithuania), "http://fra1.landx.dev");
    }

    /// 映射出的 endpoint 必须都是 HTTP 可用机房（不能落进 UDP-only 列表）。
    #[test]
    fn test_region_never_maps_to_udp_only_host() {
        let regions = [
            Region::NewYork,
            Region::Frankfurt,
            Region::Frankfurt2,
            Region::Amsterdam,
            Region::London,
            Region::Dublin,
            Region::SaltLakeCity,
            Region::Tokyo,
            Region::LosAngeles,
            Region::Pittsburgh,
            Region::Singapore,
            Region::Limburg,
            Region::Lithuania,
            Region::Unknown,
        ];
        for region in regions {
            let url = LandX::endpoint_for_region(region);
            for udp_only in LANDX_UDP_ONLY_HOSTS {
                assert!(
                    !url.contains(udp_only),
                    "region {region:?} 映射到了仅 UDP 的 {udp_only}（HTTP 必然失败）"
                );
            }
        }
    }

    /// HTTP URL：key 必须拼进 path。
    #[test]
    fn test_send_url_puts_key_in_path() {
        let c = LandX::init_with("my-key", Region::Frankfurt);
        assert_eq!(c.send_url().unwrap(), "http://fra1.landx.dev/my-key");

        let empty = LandX::init_with("", Region::Frankfurt);
        assert!(empty.send_url().is_err());
    }

    /// 最低 tip 为 0.001 SOL，平台名与 tip 收款一致。
    #[test]
    fn test_min_tip_and_platform() {
        let c = LandX::init_with("key", Region::Amsterdam);
        assert_eq!(c.get_min_tip_amount(), 1_000_000);
        assert_eq!(c.get_min_tip_amount(), LandX::MIN_TIP_AMOUNT_TX);
        assert_eq!(c.platform(), PlatformName::LandX);
        assert_eq!(c.tip_recvs().len(), 10);
        assert!(LANDX_TIP_ACCOUNTS.contains(&c.get_tip_address()));
    }

    /// 健康检查 URL。
    #[test]
    fn test_ping_endpoint_shape() {
        let ping = LandX::get_ping_endpoint();
        assert!(ping.ends_with("/ping"), "ping endpoint: {ping}");
    }
}
