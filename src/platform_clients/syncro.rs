//! P2P.ORG **Syncro Sender** 平台客户端。
//!
//! 官方文档：
//! - Getting Started / 端点与错误码：<https://docs.p2p.org/docs/syncro-sender-quick-start>
//! - Optimizing Performance（keep-alive / 重试策略）：<https://docs.p2p.org/docs/syncro-sender-optimizing-performance>
//! - Pricing, Rate Limits & Support：<https://docs.p2p.org/docs/syncro-sender-pricing-rate-limits>
//!
//! # 服务定位
//!
//! Syncro Sender 通过 **SWQoS**（Stake-Weighted Quality of Service）把交易直接投递到
//! 当前/即将出块的 validator leader，并**并行多通道**转发（单次 `sendTransaction`
//! 已经内含冗余，客户端一般不需要自己重试投递）。
//!
//! # 两种接入方式
//!
//! | | 公开端点（tip 付费） | 私有端点（API key） |
//! |---|---|---|
//! | 鉴权 | 交易内 SystemProgram transfer tip | `Authorization: Bearer <KEY>` |
//! | 最低 tip | **200,000 lamports** | **150,000 lamports** |
//! | 速率 | 1 RPS / IP | 50 RPS / client（可定制） |
//! | 支持方法 | 仅 `sendTransaction` | 全部 Solana RPC 方法 |
//!
//! 私有端点同时接受 `X-Api-Key: <KEY>`（优先级低于 `Authorization`）；本实现使用
//! 优先级更高的 `Authorization: Bearer`。
//!
//! # ⚠️ 关于 bundle：**不支持**
//!
//! 文档明确写「Supported methods: `sendTransaction` only」（公开）/「All Solana RPC
//! methods」（私有）—— `sendBundle` 并不是 Solana 标准 RPC 方法，**两种端点都没有提供**。
//! 实测（2026，三个区域 + `/` 与 `/rpc` 路径）对 `sendBundle` 均返回：
//!
//! ```json
//! {"jsonrpc":"2.0","result":null,"error":{"code":-32601,"message":"Only sendTransaction is supported"},"id":1}
//! ```
//!
//! 因此本模块**只实现 [`SendTx`](crate::platform_clients::SendTx) /
//! [`BuildTx`](crate::platform_clients::BuildTx)**，不实现
//! [`SendBundle`](crate::platform_clients::SendBundle) /
//! [`BundleSender`](crate::platform_clients::BundleSender)（否则 `send_bundle`
//! 一定会拿到 `-32601`）。若将来官方开放 bundle，只需：
//! 1. 按 `sendBundle` 的 JSON-RPC 形状补一个发送实现；
//! 2. 实现 `BundleSender`，`tip_address()` 从 [`SYNCRO_TIP_ACCOUNTS`] 随机取；
//! 3. 注意 bundle 内**至少一笔**交易要带 tip，且总 tip 达到对应端点下限。

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
use crate::platform_clients::{PlatformName, Region, TxExt};

/// 官方 9 个 tip 账户（公开/私有端点共用同一组）。
///
/// 地址同时用于：交易内 tip 收款、[`BuildTx::tip_recvs`](crate::platform_clients::BuildTx::tip_recvs)。
pub const SYNCRO_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("BPZrtYhdoAhiHWV5EgGLoV7bZFbMamBZurGDq4DmST8v"),
    pubkey!("7D5pdbkV75Sr73M1YFNZwXMed6DenwkdfbJwVWrX6drQ"),
    pubkey!("ELpn2NryEW4B3psG36eSjF45YcGMQpGGuu9J2AgAccbV"),
    pubkey!("FnckAPC9PitnRpGZM2M4WLwb3w9odRLJ7EDRZDngjvd6"),
    pubkey!("3ZnDTgvVfwzqwWoqAUmDkgVtXvXqjmeb5t9zxD5pMbmv"),
    pubkey!("3SLDFcdCzMbcFNguZhzmV4zqEAUvcPoKY13akpE4Tq1p"),
    pubkey!("48tT6LJqrsoFrLpzZSHkjGdGTWtsJ1PvjgWZjh8qF1RK"),
    pubkey!("7GM9fpVMHHcrK4cgzfVdzJvjiy1bSyfwSYzhxvgbfVLg"),
    pubkey!("CBd8GE3ffMJKf3iCCcNNBEifMxH1WpgtTzRnXPxxbjGE"),
];

/// 区域 HTTP 端点（端口 `:8080`）。
///
/// 文档把这三个端点列为「HTTP Endpoints」，私有端点直接打在根路径（或 `/rpc`）；
/// 公开端点（`/public`）由全局域名 `sfls.l2.p2p.org` 提供，见
/// [`SYNCRO_PUBLIC_ENDPOINT`]。
pub const SYNCRO_ENDPOINTS: &[&str] = &[
    "http://fra.sender.syncro.p2p.org:8080",  // Frankfurt
    "http://ams3.sender.syncro.p2p.org:8080", // Amsterdam
    "http://us.sender.syncro.p2p.org:8080",   // Washington, DC
];

/// 文档示例中的公开（tip-only）端点。
///
/// 区域端点实测不提供 `/public`（HTTP 404），公开流量请走这个全局域名。
pub const SYNCRO_PUBLIC_ENDPOINT: &str = "https://sfls.l2.p2p.org/public";

/// JSON-RPC 成功/失败响应。
#[derive(Debug, serde::Deserialize)]
struct SyncroRpcResponse {
    result: Option<String>,
    error: Option<SyncroRpcError>,
}

/// JSON-RPC 错误体。
#[derive(Debug, serde::Deserialize)]
struct SyncroRpcError {
    code: i64,
    message: String,
}

/// P2P.ORG Syncro Sender 客户端。
///
/// - **私有模式**：`api_key = Some(...)`，请求带 `Authorization: Bearer <KEY>`，
///   最低 tip 150,000 lamports。
/// - **公开模式**：`api_key = None`，不带鉴权头，最低 tip 200,000 lamports，
///   且**只允许 `sendTransaction`**。
#[derive(Clone)]
pub struct Syncro {
    /// 完整 endpoint（含 `/public`、`/rpc` 等路径，若适用）。
    pub endpoint: String,
    /// API key；`None`/空串表示公开端点。
    pub api_key: Option<String>,
    pub http_client: Arc<Client>,
}

impl Syncro {
    /// 公开端点最低 tip：200,000 lamports。
    pub const MIN_TIP_AMOUNT_PUBLIC: u64 = 200_000;
    /// 私有端点最低 tip：150,000 lamports。
    pub const MIN_TIP_AMOUNT_PRIVATE: u64 = 150_000;

    /// 按区域选区域端点；Syncro 只有 Frankfurt / Amsterdam / Washington DC 三处，
    /// 其余区域就近回退到 Frankfurt。
    pub fn regional_base(region: Region) -> &'static str {
        match region {
            Region::Amsterdam => SYNCRO_ENDPOINTS[1],
            Region::NewYork | Region::SaltLakeCity | Region::LosAngeles | Region::Pittsburgh => {
                SYNCRO_ENDPOINTS[2]
            }
            Region::Frankfurt
            | Region::Frankfurt2
            | Region::London
            | Region::Dublin
            | Region::Limburg
            | Region::Lithuania
            | Region::Tokyo
            | Region::Singapore => SYNCRO_ENDPOINTS[0],
            Region::Unknown => SYNCRO_ENDPOINTS[0],
        }
    }

    /// 供 [`crate::platform_clients::endpoint_keep_alive`] 使用的区域端点。
    pub fn get_endpoint() -> String {
        Self::regional_base(*REGION).to_string()
    }

    /// 默认构造：读到非空 `SYNCRO_API_KEY` 走私有端点，否则回退公开端点。
    pub fn new() -> Self {
        match std::env::var("SYNCRO_API_KEY").ok().filter(|k| !k.is_empty()) {
            Some(key) => Self::init_with(key, *REGION),
            None => Self::init_public(),
        }
    }

    /// 显式构造私有客户端：调用方提供 key 与 region，不读环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        Self {
            endpoint: Self::regional_base(region).to_string(),
            api_key: Some(key.into()),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 显式构造公开（tip-only）客户端，使用文档中的全局 `/public` 端点。
    pub fn init_public() -> Self {
        Self {
            endpoint: SYNCRO_PUBLIC_ENDPOINT.to_string(),
            api_key: None,
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 公开端点但指定区域 base（如自建代理/新增区域时使用）。
    pub fn init_public_at(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key: None,
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 完全自定义 endpoint（例如私有端点的 `/rpc` 路径或区域 `/public`）。
    pub fn init_with_endpoint(endpoint: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key,
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 是否走公开端点（无有效 key）。
    pub fn is_public(&self) -> bool {
        self.api_key.as_deref().is_none_or(str::is_empty)
    }

    fn get_tip_address() -> Pubkey {
        *SYNCRO_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| SYNCRO_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

impl Default for Syncro {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for Syncro {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("syncro send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
            let mut req = self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json");
            // 私有端点优先用 Authorization: Bearer（文档优先级 1）
            if let Some(key) = self.api_key.as_deref().filter(|k| !k.is_empty()) {
                req = req.header("Authorization", format!("Bearer {key}"));
            }

            let res = req
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "sendTransaction",
                    // skipPreflight: true —— 文档推荐，preflight 只增加延迟
                    "params": [tx_base64, { "encoding": "base64", "skipPreflight": true }],
                }))
                .send()
                .await;

            let resp = match res {
                Ok(resp) => resp,
                Err(e) => {
                    log::error!("syncro send error: {:?}", e);
                    return Err(format!("syncro send error: {e}"));
                }
            };

            let status = resp.status();
            // 429 时文档会带 Retry-After（秒，至少 1）
            let retry_after = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let text = resp.text().await.map_err(|e| format!("syncro response text error: {e}"))?;

            if status.as_u16() == 429 {
                return Err(format!(
                    "syncro rate limited (HTTP 429), retry after {}s: {text}",
                    retry_after.as_deref().unwrap_or("1")
                ));
            }
            if !status.is_success() {
                return Err(format!("syncro HTTP {status}: {text}"));
            }

            match serde_json::from_str::<SyncroRpcResponse>(&text) {
                Ok(parsed) => {
                    if let Some(err) = parsed.error {
                        return Err(format!("syncro error {}: {}", err.code, err.message));
                    }
                    match parsed.result {
                        Some(sig) => {
                            info!("syncro: submitted {sig}");
                            Ok(())
                        }
                        None => Err(format!("syncro unexpected response: {text}")),
                    }
                }
                Err(e) => Err(format!("syncro response parse error: {e}, raw: {text}")),
            }
        })
    }
}

impl crate::platform_clients::BuildTx for Syncro {
    fn get_tip_address(&self) -> Pubkey {
        Self::get_tip_address()
    }

    fn get_min_tip_amount(&self) -> u64 {
        if self.is_public() {
            Self::MIN_TIP_AMOUNT_PUBLIC
        } else {
            Self::MIN_TIP_AMOUNT_PRIVATE
        }
    }

    fn platform(&self) -> PlatformName {
        PlatformName::Syncro
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        SYNCRO_TIP_ACCOUNTS.to_vec()
    }

    // 默认 uses_tip_transfer() = true：公开端点必须带 tip；私有端点同样要求 tip。
    // uses_cu_price() 保持默认 true —— 文档建议同时带 priority fee 以争取调度优先。
}

impl fmt::Display for Syncro {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Syncro")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_clients::BuildTx;

    /// 官方 9 个 tip 地址，数量正确且无重复。
    #[test]
    fn test_tip_accounts_unique_and_complete() {
        assert_eq!(SYNCRO_TIP_ACCOUNTS.len(), 9);
        let mut uniq = SYNCRO_TIP_ACCOUNTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 9, "SYNCRO_TIP_ACCOUNTS 存在重复地址");
    }

    /// 区域端点映射：Amsterdam / 美东 / 其余回退 Frankfurt。
    #[test]
    fn test_endpoint_per_region() {
        assert_eq!(
            Syncro::regional_base(Region::Amsterdam),
            "http://ams3.sender.syncro.p2p.org:8080"
        );
        assert_eq!(
            Syncro::regional_base(Region::NewYork),
            "http://us.sender.syncro.p2p.org:8080"
        );
        assert_eq!(
            Syncro::regional_base(Region::Frankfurt),
            "http://fra.sender.syncro.p2p.org:8080"
        );
        // 无 Syncro 机房的区域回退 Frankfurt
        assert_eq!(
            Syncro::regional_base(Region::Tokyo),
            "http://fra.sender.syncro.p2p.org:8080"
        );
    }

    /// 公开/私有模式的最低 tip 与鉴权判定。
    #[test]
    fn test_public_private_tip_and_mode() {
        let private = Syncro::init_with("test-key", Region::Frankfurt);
        assert!(!private.is_public());
        assert_eq!(private.get_min_tip_amount(), Syncro::MIN_TIP_AMOUNT_PRIVATE);
        assert_eq!(private.get_min_tip_amount(), 150_000);
        assert_eq!(private.endpoint, "http://fra.sender.syncro.p2p.org:8080");

        let public = Syncro::init_public();
        assert!(public.is_public());
        assert_eq!(public.get_min_tip_amount(), Syncro::MIN_TIP_AMOUNT_PUBLIC);
        assert_eq!(public.get_min_tip_amount(), 200_000);
        assert_eq!(public.endpoint, SYNCRO_PUBLIC_ENDPOINT);

        // 空 key 视作公开
        let empty_key = Syncro::init_with("", Region::Amsterdam);
        assert!(empty_key.is_public());
        assert_eq!(empty_key.get_min_tip_amount(), 200_000);
    }
}
