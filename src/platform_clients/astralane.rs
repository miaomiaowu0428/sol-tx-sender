//! **Astralane** HTTP（Iris）发送器。
//!
//! 官方文档（2026-10 同步）：
//! - Endpoints and Configs：<https://astralane.gitbook.io/docs/low-latency/endpoints-and-configs>
//! - Send Txn Fee Tiers：<https://astralane.gitbook.io/docs/low-latency/send-txn-fee-tiers>
//! - QUIC：<https://astralane.gitbook.io/docs/low-latency/submit-transactions/quic-transaction-submission>
//!
//! 实现要点与端点/费率明细见 `docs/astralane.md`。
//!
//! # 端点
//!
//! 官方目前提供 **Global Edge** 与 11 个区域 Iris 端点（`fr` / `fr2` / `la` / `jp` /
//! `ny` / `ams` / `ams2` / `lim` / `sg` / `lit` / `lon`）。官方建议同时广播到
//! Global Edge 和最靠近的机房；本客户端的 [`Astralane::endpoint_for`] 仍按
//! [`Region`] 选**单个**区域端点（Global Edge 见 [`ASTRALANE_GLOBAL_ENDPOINT`]），
//! 需要双发时由调度层注册两个实例或直接覆盖 `endpoint` 字段。
//!
//! # 最低 tip
//!
//! 官方按滚动 7 天 tip 量分档：Free 0.001 SOL、VIP 1/2 0.0001 SOL、VIP 3
//! 0.00001 SOL。本实现沿用 **VIP 3** 档位值（见 [`Astralane::MIN_TIP_AMOUNT_TX`]）。

use std::fmt;
impl fmt::Display for Astralane {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Astralane")
    }
}

/// Astralane sendBundle 响应结构体
#[derive(Debug, serde::Deserialize)]
struct AstralaneSendBundleResponse {
    pub _jsonrpc: Option<String>,
    pub result: Option<Vec<String>>,
    pub error: Option<serde_json::Value>,
    pub _id: Option<u64>,
}

use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use utils::log_time;

use crate::platform_clients::{BuildTx, TxExt};
use solana_sdk::signature::Signature;
use solana_sdk::transaction::VersionedTransaction;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region};

/// 官方 tip 地址（8 个基础地址 + 9 个 Recently Added，共 17 个）。
///
/// 地址同时用于：交易内 tip 收款、[`BuildTx::tip_recvs`]。
pub const ASTRALANE_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("astrazznxsGUhWShqgNtAdfrzP2G83DzcWVJDxwV9bF"),
    pubkey!("astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm"),
    pubkey!("astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk"),
    pubkey!("astraRVUuTHjpwEVvNBeQEgwYx9w9CFyfxjYoobCZhL"),
    pubkey!("astraEJ2fEj8Xmy6KLG7B3VfbKfsHXhHrNdCQx7iGJK"),
    pubkey!("astraubkDw81n4LuutzSQ8uzHCv4BhPVhfvTcYv8SKC"),
    pubkey!("astraZW5GLFefxNPAatceHhYjfA1ciq9gvfEg2S47xk"),
    pubkey!("astrawVNP4xDBKT7rAdxrLYiTSTdqtUr63fSMduivXK"),
    // Recently Added（2026-10 文档新增）
    pubkey!("AstrA1ejL4UeXC2SBP4cpeEmtcFPZVLxx3XGKXyCW6to"),
    pubkey!("AsTra79FET4aCKWspPqeSFvjJNyp96SvAnrmyAxqg5b7"),
    pubkey!("AstrABAu8CBTyuPXpV4eSCJ5fePEPnxN8NqBaPKQ9fHR"),
    pubkey!("AsTRADtvb6tTmrsqULQ9Wji9PigDMjhfEMza6zkynEvV"),
    pubkey!("AsTRAEoyMofR3vUPpf9k68Gsfb6ymTZttEtsAbv8Bk4d"),
    pubkey!("AStrAJv2RN2hKCHxwUMtqmSxgdcNZbihCwc1mCSnG83W"),
    pubkey!("Astran35aiQUF57XZsmkWMtNCtXGLzs8upfiqXxth2bz"),
    pubkey!("AStRAnpi6kFrKypragExgeRoJ1QnKH7pbSjLAKQVWUum"),
    pubkey!("ASTRaoF93eYt73TYvwtsv6fMWHWbGmMUZfVZPo3CRU9C"),
];

/// Global Edge 端点：官方建议与最近区域端点**同时**广播。
///
/// 本客户端不会自动双发，需要时由调用方额外注册一个实例并把 `endpoint`
/// 覆盖为该值（或直接构造等价的 [`Astralane`]）。
pub const ASTRALANE_GLOBAL_ENDPOINT: &str = "https://edge.astralane.io/iris";

/// 官方区域 Iris 端点（`/iris?api-key=...` 由请求时拼接）。
///
/// 顺序与 [`Astralane::endpoint_for`] 的映射保持一致，便于测试与遍历。
/// `la` 对应官方文档中的 Los Angeles（旧文档曾称 San Francisco，旧 `lax`
/// 域名已废弃）。
pub const ASTRALANE_ENDPOINTS: &[&str] = &[
    "http://fr.gateway.astralane.io/iris",   // Frankfurt（recommended）
    "http://fr2.gateway.astralane.io/iris",  // Frankfurt 2
    "http://la.gateway.astralane.io/iris",   // Los Angeles
    "http://jp.gateway.astralane.io/iris",   // Tokyo
    "http://ny.gateway.astralane.io/iris",   // New York
    "http://ams.gateway.astralane.io/iris",  // Amsterdam（recommended）
    "http://ams2.gateway.astralane.io/iris", // Amsterdam 2（Cherry Servers）
    "http://lim.gateway.astralane.io/iris",  // Limburg
    "http://sg.gateway.astralane.io/iris",   // Singapore
    "http://lit.gateway.astralane.io/iris",  // Lithuania
    "http://lon.gateway.astralane.io/iris",  // London
];

#[derive(Clone)]
pub struct Astralane {
    pub endpoint: String,   // 只保存基础 endpoint，不拼 key
    pub auth_token: String, // 单独保存 key
    pub http_client: Arc<Client>,
}

impl Astralane {
    /// 单笔交易最低 tip：0.00001 SOL（10,000 lamports）。
    ///
    /// 官方按滚动 7 天 tip 量分档：Free 0.001 SOL、VIP 1/2 0.0001 SOL、VIP 3
    /// 0.00001 SOL。这里沿用 **VIP 3** 档位；若账号档位更低需自行调高。
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_000_010_000;
    /// 批量交易最低 tip：0.0001 SOL（100,000 lamports，对应 VIP 1/2 档）。
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 0_000_100_000;

    /// 根据 Region 返回对应的 endpoint URL（纯函数，不依赖全局状态）。
    ///
    /// 没有 Astralane 机房的区域就近回退：
    /// - `Dublin` → London
    /// - `SaltLakeCity` → Los Angeles
    /// - `Pittsburgh` → New York
    /// - `Unknown` → Frankfurt
    pub fn endpoint_for(region: Region) -> &'static str {
        match region {
            Region::Frankfurt => "http://fr.gateway.astralane.io/iris",
            Region::Frankfurt2 => "http://fr2.gateway.astralane.io/iris",
            Region::LosAngeles => "http://la.gateway.astralane.io/iris",
            Region::Tokyo => "http://jp.gateway.astralane.io/iris",
            Region::NewYork | Region::Pittsburgh => "http://ny.gateway.astralane.io/iris",
            Region::Amsterdam => "http://ams.gateway.astralane.io/iris",
            Region::Limburg => "http://lim.gateway.astralane.io/iris",
            Region::Singapore => "http://sg.gateway.astralane.io/iris",
            Region::Lithuania => "http://lit.gateway.astralane.io/iris",
            Region::London | Region::Dublin => "http://lon.gateway.astralane.io/iris",
            Region::SaltLakeCity => "http://la.gateway.astralane.io/iris",
            Region::Unknown => "http://fr.gateway.astralane.io/iris",
        }
    }

    /// 兼容旧调用：从 `REGION` 全局 static 读取地区。
    pub fn get_endpoint() -> String {
        Self::endpoint_for(*REGION).to_string()
    }

    /// 显式构造：调用方负责提供 key 和 region，不读取任何环境变量。
    /// 单例还是每次 new，由业务侧决定。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        Astralane {
            endpoint: Self::endpoint_for(region).to_string(),
            auth_token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 兼容旧调用：从 `REGION` 全局 static + `ASTRALANE_KEY` 环境变量读取。
    pub fn new() -> Self {
        let endpoint = Self::endpoint_for(*REGION).to_string();
        let auth_token = std::env::var("ASTRALANE_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        Astralane {
            endpoint,
            auth_token,
            http_client,
        }
    }

    /// 生成 BundleSender 注入到 BundleBuilder
    pub fn bundle_sender() -> Box<dyn crate::platform_clients::BundleSender> {
        Box::new(Astralane::new())
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::BundleSender for Astralane {
    fn tip_address(&self) -> Pubkey {
        self.get_tip_address()
    }

    fn max_tx_size(&self) -> usize {
        // V1 交易上限 4096 字节（SIMD-0296）。V0 交易实际不会超过此值。
        4096
    }

    async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<Vec<Signature>, String> {
        // 构建 IrisB sendBatch 二进制 body: [u16 BE len][wire tx]...
        // ⚠️ 必须是 **Solana wire 格式**（wincode），不能用 bincode ——
        //    后者会把 signatures 的长度前缀写成 u64 而非 short_vec。
        let mut body = Vec::new();
        let mut sigs = Vec::with_capacity(txs.len());
        for tx in txs {
            let tx_bytes = crate::platform_clients::serialize_transaction_wire(tx)?;
            let len = tx_bytes.len() as u16;
            body.extend_from_slice(&len.to_be_bytes());
            body.extend_from_slice(&tx_bytes);
            sigs.push(tx.sig());
        }

        let base = self.endpoint.strip_suffix("/iris").unwrap_or(&self.endpoint);
        let url = format!("{base}/irisb?api-key={}&method=sendBatch", self.auth_token);
        let response = self
            .http_client
            .post(&url)
            .header("content-type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("send: {e}"))?;

        let status = response.status();
        let body_text = response.text().await.map_err(|e| format!("read: {e}"))?;

        if !status.is_success() {
            return Err(format!("irisb sendBatch failed ({status}): {body_text}"));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| format!("parse response: {e}"))?;
        let sig_array = parsed["result"]
            .as_array()
            .or_else(|| parsed.as_array())
            .ok_or_else(|| format!("unexpected response format: {body_text}"))?;

        log::info!("[astralane/irisb] sent {} txs, {} sigs", txs.len(), sig_array.len());
        Ok(sigs)
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for Astralane {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("astralane send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
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
            let res = self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .header("api-key", self.auth_token.as_str())
                .json(&req_json)
                .send()
                .await;
            let response = match res {
                Ok(resp) => match resp.text().await {
                    Ok(text) => text,
                    Err(e) => {
                        return Err(format!("response text error: {}", e));
                    }
                },
                Err(e) => {
                    return Err(format!("send error: {}", e));
                }
            };
            info!("[astralane/send_tx] response: {}", response);
            Ok(())
        })
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Astralane {
    async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<Vec<Signature>, String> {
        // 将所有交易序列化并 base64 编码
        let mut encoded_txs = Vec::with_capacity(txs.len());
        let mut sigs: Vec<Signature> = Vec::with_capacity(txs.len());
        for tx in txs {
            let encode_tx = match crate::platform_clients::serialize_transaction_wire_base64(tx) {
                Ok(s) => s,
                Err(e) => return Err(format!("wire serialize error: {e}")),
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
            Err(e) => Err(format!("astralane response parse error: {}, raw: {}", e, response)),
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
    fn tip_recvs(&self) -> Vec<Pubkey> {
        ASTRALANE_TIP_ACCOUNTS.to_vec()
    }

    // 使用默认实现，无需重写 build_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 tip 地址共 17 个（8 基础 + 9 Recently Added），且不能有重复/漏抄。
    #[test]
    fn tip_accounts_complete_and_unique() {
        assert_eq!(ASTRALANE_TIP_ACCOUNTS.len(), 17);
        let mut uniq = ASTRALANE_TIP_ACCOUNTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 17, "ASTRALANE_TIP_ACCOUNTS 存在重复地址");
    }

    /// 每个 Region 都映射到官方区域端点；没有机房的区域按注释就近回退。
    #[test]
    fn endpoint_per_region_maps_to_official_list() {
        assert_eq!(ASTRALANE_ENDPOINTS.len(), 11);
        assert!(ASTRALANE_GLOBAL_ENDPOINT.ends_with("/iris"));

        let cases = [
            (Region::Frankfurt, "http://fr.gateway.astralane.io/iris"),
            (Region::Frankfurt2, "http://fr2.gateway.astralane.io/iris"),
            (Region::LosAngeles, "http://la.gateway.astralane.io/iris"),
            (Region::Tokyo, "http://jp.gateway.astralane.io/iris"),
            (Region::NewYork, "http://ny.gateway.astralane.io/iris"),
            (Region::Amsterdam, "http://ams.gateway.astralane.io/iris"),
            (Region::Limburg, "http://lim.gateway.astralane.io/iris"),
            (Region::Singapore, "http://sg.gateway.astralane.io/iris"),
            (Region::Lithuania, "http://lit.gateway.astralane.io/iris"),
            (Region::London, "http://lon.gateway.astralane.io/iris"),
            // 就近回退
            (Region::Dublin, "http://lon.gateway.astralane.io/iris"),
            (Region::SaltLakeCity, "http://la.gateway.astralane.io/iris"),
            (Region::Pittsburgh, "http://ny.gateway.astralane.io/iris"),
            (Region::Unknown, "http://fr.gateway.astralane.io/iris"),
        ];
        for (region, expected) in cases {
            let actual = Astralane::endpoint_for(region);
            assert_eq!(actual, expected, "region {region:?}");
            assert!(ASTRALANE_ENDPOINTS.contains(&actual), "endpoint 未收录: {actual}");
        }
    }
}
