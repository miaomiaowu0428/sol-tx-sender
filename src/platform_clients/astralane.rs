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
//! # 路由开关（重要）
//!
//! 官方两个可选参数都会**收窄**一条交易被分发的分支数：
//!
//! | 参数 | 官方默认 | 本客户端 | 置 true 的后果 |
//! | --- | --- | --- | --- |
//! | `swqos-only` | `false` | **显式 false** | 只走 SWqoS（Jito / Paladin），放弃 staked peer / TPU 直连 |
//! | `mev-protect`（JSON `mevProtect`） | `false` | **显式 false** | 收敛到受保护的私有路径子集 |
//!
//! 两者都写死在 [`SUBMIT_URL_QUERY`] / 请求体里，**不依赖平台默认值**：默认值一旦
//! 变更，路由分支会静默变少且没有任何日志。要"尽可能多走路由分支"就别改它们。
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

/// 提交 URL 上**显式关闭**的路由收敛开关。
///
/// 官方两个可选参数都会**收窄**路由集合（即减少一条交易被分发的分支数）：
///
/// - `swqos-only`（默认 `false`）：置 `true` 时交易**只**走 SWQoS（Jito / Paladin），
///   放弃 staked peer / TPU 直连等其它分支。
/// - `mev-protect`（默认 `false`）：置 `true` 时走受保护的私有路径集合，同样是
///   路由子集。
///
/// 本客户端的目标是「尽可能多路由分支」，所以两个参数都写成 `false`，并且
/// **显式拼在 URL 上**而不是依赖平台默认值——避免对方默认值变更时静默减少分支。
pub const SUBMIT_URL_QUERY: &str = "swqos-only=false&mev-protect=false";

/// Global Edge 端点：官方建议与最近区域端点**同时**广播。
pub const ASTRALANE_GLOBAL_ENDPOINT: &str = "https://edge.astralane.io/iris";

/// 端点别名 → URL，供 `ASTRALANE_FANOUT` 使用（短名、可读、可配）。
///
/// 官方每个地区有 1~2 个机房（`fr`+`fr2`、`ams`+`ams2`），外加一个 Global Edge。
/// 别名与 [`Astralane::endpoint_for`] 使用的是同一批 URL。
pub const ASTRALANE_HOSTS: &[(&str, &str)] = &[
    ("edge", ASTRALANE_GLOBAL_ENDPOINT),
    ("fr", "http://fr.gateway.astralane.io/iris"),
    ("fr2", "http://fr2.gateway.astralane.io/iris"),
    ("la", "http://la.gateway.astralane.io/iris"),
    ("jp", "http://jp.gateway.astralane.io/iris"),
    ("ny", "http://ny.gateway.astralane.io/iris"),
    ("ams", "http://ams.gateway.astralane.io/iris"),
    ("ams2", "http://ams2.gateway.astralane.io/iris"),
    ("lim", "http://lim.gateway.astralane.io/iris"),
    ("sg", "http://sg.gateway.astralane.io/iris"),
    ("lit", "http://lit.gateway.astralane.io/iris"),
    ("lon", "http://lon.gateway.astralane.io/iris"),
];

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

/// Astralane HTTP 客户端。
///
/// # 多端点扇出（`endpoints`）
///
/// 一个实例可以持有**多个入口**，`send_tx` 会把**同一笔已签名交易**并发打到所有
/// 入口。这样做是有意的：
///
/// - 交易字节只有一份 ⇒ **签名相同** ⇒ 服务端看到的是「同一笔交易走了 N 个入口」，
///   对应 Inspector 里**同一个签名下的 N 个 bundle**（这才是我们想要的形态）。
/// - 如果改成在上层「多发几次」，每次都会重新 build（tip 账户随机）⇒ 变成 N 笔
///   **不同签名**的交易，只会得到 N 行、每行 1 个 bundle。
///
/// 默认只有本地区 1 个入口；用 `ASTRALANE_FANOUT` 打开（见
/// [`Astralane::parse_fanout`]）。
#[derive(Clone)]
pub struct Astralane {
    /// 基础 endpoint 列表（**不含** query，key 走请求头）。
    ///
    /// 至少 1 个；顺序 = 主入口在前。
    pub endpoints: Vec<String>,
    pub auth_token: String,
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

    /// 给某个基础 endpoint 拼上 [`SUBMIT_URL_QUERY`]。
    ///
    /// `api-key` 仍走请求头（与历史实现一致），所以这里只拼路由开关。
    /// 对已经带 `?` 的端点做兼容，避免拼出两个 `?`。
    pub fn submit_url_for(endpoint: &str) -> String {
        let sep = if endpoint.contains('?') { '&' } else { '?' };
        format!("{endpoint}{sep}{SUBMIT_URL_QUERY}")
    }

    /// 主入口（本地区）的提交 URL。
    pub fn submit_url(&self) -> String {
        Self::submit_url_for(self.primary())
    }

    /// 主入口（列表第一个）。
    pub fn primary(&self) -> &str {
        self.endpoints.first().map(|s| s.as_str()).unwrap_or("")
    }

    /// 解析 `ASTRALANE_FANOUT` 规格，返回**额外的**基础 endpoint（不含主入口）。
    ///
    /// 支持的写法：
    ///
    /// | 值 | 含义 |
    /// | --- | --- |
    /// | 空 / `off` / `0` | 不扇出（默认） |
    /// | `edge` | 只加 Global Edge（**官方推荐**：edge + 最近机房） |
    /// | `all` | 全部 12 个入口（11 机房 + edge）⚠️ 见限速说明 |
    /// | `edge,fr2` / `ams2,lim` | 逗号分隔的别名，见 [`ASTRALANE_HOSTS`] |
    ///
    /// ⚠️ **扇出会等比例消耗 TPS**：Astralane 的 TPS 限额是按账号的（Free 5、
    /// VIP1 20、VIP2 40、VIP3 80）。打到 N 个入口 = 占用 N 份单笔配额，还可能拉低
    /// bundle 成功率（N 份里只有 1 份会落地），进而触发 QoS 惩罚。
    /// 所以默认关闭，建议从 `edge`（2 个入口）起步。
    pub fn parse_fanout(spec: &str) -> Result<Vec<String>, String> {
        let spec = spec.trim();
        if spec.is_empty() || spec.eq_ignore_ascii_case("off") || spec == "0" {
            return Ok(Vec::new());
        }
        if spec.eq_ignore_ascii_case("all") {
            return Ok(ASTRALANE_HOSTS.iter().map(|(_, url)| url.to_string()).collect());
        }

        let mut out = Vec::new();
        for token in spec.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()) {
            if token.contains("://") {
                out.push(token.to_string());
                continue;
            }
            let url = ASTRALANE_HOSTS
                .iter()
                .find(|(alias, _)| alias.eq_ignore_ascii_case(token))
                .map(|(_, url)| *url)
                .ok_or_else(|| {
                    let known: Vec<&str> = ASTRALANE_HOSTS.iter().map(|(a, _)| *a).collect();
                    format!(
                        "未知端点别名 `{token}`（可用: off | all | {}）",
                        known.join(", ")
                    )
                })?;
            out.push(url.to_string());
        }
        Ok(out)
    }

    /// 按 region + `ASTRALANE_FANOUT` 算出最终入口列表（去重，主入口在前）。
    ///
    /// 环境变量写错时**不 panic**：打一条 warn 后回退到单入口，别让一个拼写错误
    /// 把发送通道打挂。
    pub fn endpoints_for(region: Region) -> Vec<String> {
        let primary = Self::endpoint_for(region).to_string();
        // 默认开启：官方推荐的「Global Edge + 最近机房」双发。
        let spec = std::env::var("ASTRALANE_FANOUT").unwrap_or_else(|_| "edge".to_string());

        let mut list = vec![primary];
        match Self::parse_fanout(&spec) {
            Ok(extra) => {
                for ep in extra {
                    if !list.contains(&ep) {
                        list.push(ep);
                    }
                }
            }
            Err(e) => log::warn!("[astralane] ASTRALANE_FANOUT 解析失败，回退单入口: {e}"),
        }
        list
    }

    /// 显式构造：调用方负责提供 key 和 region，入口列表由 [`Self::endpoints_for`]
    /// 决定（即受 `ASTRALANE_FANOUT` 控制）。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        Self::init_with_endpoints(key, Self::endpoints_for(region))
    }

    /// 完全显式：直接给定入口列表（第一个视为主入口）。
    pub fn init_with_endpoints(key: impl Into<String>, endpoints: Vec<String>) -> Self {
        assert!(!endpoints.is_empty(), "Astralane 至少需要 1 个 endpoint");
        Astralane {
            endpoints,
            auth_token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 兼容旧调用：从 `REGION` 全局 static + `ASTRALANE_KEY` 环境变量读取。
    pub fn new() -> Self {
        Astralane {
            endpoints: Self::endpoints_for(*REGION),
            auth_token: std::env::var("ASTRALANE_KEY").unwrap_or_default(),
            http_client: HTTP_CLIENT.clone(),
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

        let primary = self.primary();
        let base = primary.strip_suffix("/iris").unwrap_or(primary);
        let url = format!(
            "{base}/irisb?api-key={}&method=sendBatch&{SUBMIT_URL_QUERY}",
            self.auth_token
        );
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
                    // ⚠️ 必须 false：true 会把路由收敛到受保护的私有子集（分支更少）。
                    { "mevProtect": false }
                ],
            });
            // 并发打到所有入口：**同一份 tx_base64**（同一签名）。
            // 只要有一个入口 2xx 就算发出去了；全失败才返回错误。
            let mut set = tokio::task::JoinSet::new();
            for endpoint in &self.endpoints {
                let client = self.http_client.clone();
                let url = Self::submit_url_for(endpoint);
                let key = self.auth_token.clone();
                let body = req_json.clone();
                let label = endpoint.clone();
                set.spawn(async move {
                    let res = client
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .header("api-key", key.as_str())
                        .json(&body)
                        .send()
                        .await;
                    match res {
                        Ok(resp) => {
                            let status = resp.status();
                            let text = resp.text().await.unwrap_or_default();
                            (label, status.is_success(), format!("HTTP {status} {text}"))
                        }
                        Err(e) => (label, false, format!("send error: {e}")),
                    }
                });
            }

            let mut ok = 0usize;
            let mut errors: Vec<String> = Vec::new();
            while let Some(joined) = set.join_next().await {
                match joined {
                    Ok((label, success, detail)) => {
                        if success {
                            ok += 1;
                            info!("[astralane/send_tx] {label} ok: {detail}");
                        } else {
                            log::warn!("[astralane/send_tx] {label} failed: {detail}");
                            errors.push(format!("{label}: {detail}"));
                        }
                    }
                    Err(e) => errors.push(format!("task join error: {e}")),
                }
            }

            if ok == 0 {
                return Err(format!(
                    "all {} endpoint(s) failed: {}",
                    self.endpoints.len(),
                    errors.join(" | ")
                ));
            }
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
                    // ⚠️ 必须 false，理由同 send_tx：收敛路由 = 减少分支。
                    "mevProtect": false,
                    "revertProtection": false
                }
            ]
        })) {
            Ok(body) => body,
            Err(e) => return Err(format!("serde_json error: {}", e)),
        };

        let res = self
            .http_client
            .post(&self.submit_url())
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

    /// 提交 URL 必须**显式**关掉两个收窄路由的开关。
    ///
    /// 一旦有人把 `swqos-only` / `mev-protect` 改回 true（或删掉），这条测试会红：
    /// 它们都会让交易走更少的分支，与「尽可能多路由分支」的目标相反。
    #[test]
    fn submit_url_explicitly_disables_route_narrowing_flags() {
        let c = Astralane::init_with("test-key", Region::Frankfurt);
        let url = c.submit_url();
        assert!(url.contains("swqos-only=false"), "{url}");
        assert!(url.contains("mev-protect=false"), "{url}");
        assert!(!url.contains("=true"), "{url}");
        // 只有一个 '?'，且拼在 /iris 之后
        assert_eq!(url.matches('?').count(), 1, "{url}");
        assert!(url.starts_with("http://fr.gateway.astralane.io/iris?"), "{url}");
        // api-key 走请求头，不能出现在 URL 里
        assert!(!url.contains("api-key"), "{url}");
    }

    /// endpoint 自带 `?` 时用 `&` 拼接，不产生第二个 `?`。
    #[test]
    fn submit_url_handles_preset_query() {
        let url = Astralane::submit_url_for("https://edge.astralane.io/iris?foo=1");
        assert_eq!(url.matches('?').count(), 1, "{url}");
        assert!(url.contains("?foo=1&swqos-only=false"), "{url}");
    }

    /// 扇出规格解析：默认关闭，`edge` 只加 edge，`all` 全量，未知别名必须报错。
    #[test]
    fn parse_fanout_spec() {
        assert!(Astralane::parse_fanout("").unwrap().is_empty());
        assert!(Astralane::parse_fanout("off").unwrap().is_empty());
        assert!(Astralane::parse_fanout("OFF").unwrap().is_empty());
        assert!(Astralane::parse_fanout("0").unwrap().is_empty());

        let edge = Astralane::parse_fanout("edge").unwrap();
        assert_eq!(edge, vec![ASTRALANE_GLOBAL_ENDPOINT.to_string()]);

        assert_eq!(Astralane::parse_fanout("all").unwrap().len(), ASTRALANE_HOSTS.len());

        let two = Astralane::parse_fanout("edge, fr2").unwrap();
        assert_eq!(two.len(), 2, "逗号两侧空白要能容忍");
        assert!(two[1].contains("fr2.gateway"));

        // 直接写完整 URL 也接受
        let raw = Astralane::parse_fanout("http://custom.example/iris").unwrap();
        assert_eq!(raw, vec!["http://custom.example/iris".to_string()]);

        // 拼错必须报错，不能静默变成单入口
        let err = Astralane::parse_fanout("fr3").unwrap_err();
        assert!(err.contains("fr3"), "{err}");
        assert!(err.contains("off"), "错误信息里应列出可用别名: {err}");
    }

    /// 入口列表：主入口始终在第一位，且不重复。
    #[test]
    fn endpoints_for_puts_primary_first_and_dedups() {
        // 不依赖环境变量：这里只验证去重逻辑本身
        let list = Astralane::endpoints_for(Region::Frankfurt);
        assert_eq!(list[0], Astralane::endpoint_for(Region::Frankfurt));
        let mut uniq = list.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), list.len(), "入口列表有重复: {list:?}");
    }

    /// 显式入口列表：第一个是主入口。
    #[test]
    fn init_with_endpoints_uses_first_as_primary() {
        let c = Astralane::init_with_endpoints(
            "k",
            vec![
                "http://fr.gateway.astralane.io/iris".to_string(),
                ASTRALANE_GLOBAL_ENDPOINT.to_string(),
            ],
        );
        assert_eq!(c.endpoints.len(), 2);
        assert_eq!(c.primary(), "http://fr.gateway.astralane.io/iris");
        assert!(c.submit_url().starts_with("http://fr.gateway.astralane.io/iris?"));
        assert_eq!(Astralane::endpoint_for(Region::Frankfurt), c.primary());
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
