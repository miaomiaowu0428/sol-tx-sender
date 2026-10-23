//! **Temporal (Nozomi)** 发送器。
//!
//! 两条提交路径，权威文档见 <https://use.temporal.xyz/nozomi/batch-send>：
//!
//! | 路径 | 端点 | 特点 |
//! | --- | --- | --- |
//! | **Batch Send**（默认） | `POST /api/sendBatch?c=<key>`，`application/octet-stream` | 文档原话：「lowest-overhead submission path…fastest option **even for a single transaction**」 |
//! | JSON-RPC（回落） | `POST /?c=<key>`，`sendTransaction` + base64 JSON | 兼容老路径，会返回签名 |
//!
//! # 为什么默认走 Batch Send
//!
//! Nozomi 文档明确建议「在**直连 `http://` 端点 + 热连接**上发 batch」，并且说
//! QUIC 通道（H3）只有在**无法保持长连接**时才划算 —— 我们进程长活、`HTTP_CLIENT`
//! 又做了连接池 + keep-alive，所以这里不需要 H3，只需要换掉 JSON 信封。
//!
//! # Batch Send 的语义（和别的平台不一样）
//!
//! - 成功是 **200 + 空 body**；失败是纯文本 `400/401/429/500`。
//! - **不返回交易签名**，签名本来就要客户端自己算（我们有 `tx.sig()`）。
//! - **流式处理、可能部分成功且不回滚**：第 N 笔失败时前 N-1 笔可能已经受理。
//!
//! 因此本实现在 Batch Send 返回非 2xx 时**回落一次 JSON-RPC**：
//! 路径不通（404/415）也能正常发出去；错误日志里能看到真实状态码。想强制老路径
//! 可设 `TEMPORAL_JSON_RPC=1`。
use log::{info, warn};
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use utils::log_time;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region, TxExt, serialize_transaction_wire};
use solana_sdk::transaction::VersionedTransaction;

pub const TEMPORAL_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq"),
    pubkey!("noz3jAjPiHuBPqiSPkkugaJDkJscPuRhYnSpbi8UvC4"),
    pubkey!("noz3str9KXfpKknefHji8L1mPgimezaiUyCHYMDv1GE"),
    pubkey!("noz6uoYCDijhu1V7cutCpwxNiSovEwLdRHPwmgCGDNo"),
    pubkey!("noz9EPNcT7WH6Sou3sr3GGjHQYVkN3DNirpbvDkv9YJ"),
    pubkey!("nozc5yT15LazbLTFVZzoNZCwjh3yUtW86LoUyqsBu4L"),
    pubkey!("nozFrhfnNGoyqwVuwPAW4aaGqempx4PU6g6D9CJMv7Z"),
    pubkey!("nozievPk7HyK1Rqy1MPJwVQ7qQg2QoJGyP71oeDwbsu"),
    pubkey!("noznbgwYnBLDHu8wcQVCEw6kDrXkPdKkydGJGNXGvL7"),
    pubkey!("nozNVWs5N8mgzuD3qigrCG2UoKxZttxzZ85pvAQVrbP"),
    pubkey!("nozpEGbwx4BcGp6pvEdAh1JoC2CQGZdU6HbNP1v2p6P"),
    pubkey!("nozrhjhkCr3zXT3BiT4WCodYCUFeQvcdUkM7MqhKqge"),
    pubkey!("nozrwQtWhEdrA6W8dkbt9gnUaMs52PdAv5byipnadq3"),
    pubkey!("nozUacTVWub3cL4mJmGCYjKZTnE9RbdY5AP46iQgbPJ"),
    pubkey!("nozWCyTPppJjRuw2fpzDhhWbW355fzosWSzrrMYB1Qk"),
    pubkey!("nozWNju6dY353eMkMqURqwQEoM3SFgEKC6psLCSfUne"),
    pubkey!("nozxNBgWohjR75vdspfxR5H9ceC7XXH99xpxhVGt3Bb"),
];

pub const TEMPORAL_ENDPOINT: &[&str] = &[
    "http://pit1.nozomi.temporal.xyz/", // AMS
    "http://tyo1.nozomi.temporal.xyz/", // Tokyo
    "http://sgp1.nozomi.temporal.xyz/", // sg
    "http://ewr1.nozomi.temporal.xyz/", // NY
    "http://ams1.nozomi.temporal.xyz/", // Amsterdam
    "http://fra2.nozomi.temporal.xyz/", //Frankfurt
];

// ── Batch Send（`/api/sendBatch`）上限，来自官方文档 ──────────────────────
// <https://use.temporal.xyz/nozomi/batch-send>
/// 一个 batch 最多多少笔。
pub const MAX_BATCH_TXS: usize = 16;
/// Legacy / V0 单笔上限。
pub const MAX_TX_SIZE_LEGACY: usize = 1232;
/// V1 单笔上限。
pub const MAX_TX_SIZE_V1: usize = 4096;
/// 单笔下限（官方 66 字节）。
pub const MIN_TX_SIZE: usize = 66;
/// 整个 body 上限。
pub const MAX_BATCH_BODY: usize = 65_568;
/// SIMD-0385：V1 wire 交易的第 0 字节判别位。
const V1_DISCRIMINATOR: u8 = 0x81;

#[derive(Clone)]
pub struct Temporal {
    pub endpoint: String,
    pub token: String,
    pub http_client: Arc<Client>,
}

// impl
impl Temporal {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip

    /// 按区域返回入口 URL（纯函数，不依赖全局状态）。
    ///
    /// Temporal 没有机房的区域就近回退：
    /// - `Frankfurt2` → Frankfurt
    /// - `London` / `Dublin` / `Limburg` / `Lithuania` → Amsterdam
    /// - `LosAngeles` / `Pittsburgh` / `SaltLakeCity` → New York
    /// - `Unknown` → Frankfurt
    pub fn endpoint_for(region: Region) -> &'static str {
        match region {
            Region::Frankfurt | Region::Frankfurt2 => TEMPORAL_ENDPOINT[5],
            Region::Amsterdam
            | Region::London
            | Region::Dublin
            | Region::Limburg
            | Region::Lithuania => TEMPORAL_ENDPOINT[4],
            Region::NewYork | Region::LosAngeles | Region::Pittsburgh | Region::SaltLakeCity => {
                TEMPORAL_ENDPOINT[3]
            }
            Region::Tokyo => TEMPORAL_ENDPOINT[1],
            Region::Singapore => TEMPORAL_ENDPOINT[2],
            Region::Unknown => TEMPORAL_ENDPOINT[5],
        }
    }

    /// 兼容旧调用：从 `REGION` 全局 static 读取地区。
    ///
    /// 注意：不再对未知地区返回空串（旧实现返回 `""`，导致每笔发送都打到非法
    /// URL）。现在统一走 [`Self::endpoint_for`] 的就近回退。
    pub fn get_endpoint() -> String {
        Self::endpoint_for(*REGION).to_string()
    }

    pub fn new() -> Self {
        let endpoint = Self::get_endpoint();
        let token = std::env::var("TEMPORAL_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        Temporal {
            endpoint,
            token,
            http_client,
        }
    }

    /// 显式构造：调用方负责提供 key 和 region，不读取任何环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        let endpoint = Self::endpoint_for(region).to_string();
        Temporal {
            endpoint,
            token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    // ── Batch Send（`/api/sendBatch`）─────────────────────────────────────────

    /// Batch Send 的提交 URL：`{endpoint}/api/sendBatch?c={key}`。
    ///
    /// 端点常量自带结尾 `/`，这里统一去掉再拼，避免出现 `//api/...`。
    pub fn batch_url(&self) -> String {
        format!(
            "{}/api/sendBatch?c={}",
            self.endpoint.trim_end_matches('/'),
            self.token
        )
    }

    /// 是否强制走老 JSON-RPC 路径（`TEMPORAL_JSON_RPC=1`）。
    ///
    /// 用途：万一 `/api/sendBatch` 在某个区域不可用/行为异常，不用改代码就能退回。
    pub fn force_json_rpc() -> bool {
        matches!(
            std::env::var("TEMPORAL_JSON_RPC").as_deref(),
            Ok("1") | Ok("true")
        )
    }

    /// 单笔交易的字节上限：V1 是 4096，其余（legacy/v0）是 1232。
    ///
    /// 判定方式与 wire 格式一致（V1 判别字节 0x81 在第 0 位，见 `tx_wire_format` 测试）。
    pub fn max_tx_size_for(wire: &[u8]) -> usize {
        match wire.first() {
            Some(&V1_DISCRIMINATOR) => MAX_TX_SIZE_V1,
            _ => MAX_TX_SIZE_LEGACY,
        }
    }

    /// 编码 batch body：`[len_hi][len_lo][tx_bytes...]...`，长度前缀是 **u16 BE**。
    ///
    /// 校验顺序照文档：笔数 → 单笔大小 → 总大小。任何一项不满足都直接报错，
    /// 不做截断（截断会被服务端按 framing error 整批拒掉）。
    pub fn encode_batch(txs: &[&[u8]]) -> Result<Vec<u8>, String> {
        if txs.is_empty() {
            return Err("batch 为空".to_string());
        }
        if txs.len() > MAX_BATCH_TXS {
            return Err(format!("batch 有 {} 笔，超过上限 {MAX_BATCH_TXS}", txs.len()));
        }

        let mut body = Vec::with_capacity(txs.iter().map(|t| t.len() + 2).sum());
        for tx in txs {
            if tx.len() < MIN_TX_SIZE {
                return Err(format!("单笔 {} 字节，低于下限 {MIN_TX_SIZE}", tx.len()));
            }
            let max = Self::max_tx_size_for(tx);
            if tx.len() > max {
                return Err(format!("单笔 {} 字节，超过该版本上限 {max}", tx.len()));
            }
            let len = u16::try_from(tx.len()).map_err(|_| format!("单笔 {} 字节超过 u16", tx.len()))?;
            body.extend_from_slice(&len.to_be_bytes());
            body.extend_from_slice(tx);
        }

        if body.len() > MAX_BATCH_BODY {
            return Err(format!("batch body {} 字节，超过上限 {MAX_BATCH_BODY}", body.len()));
        }
        Ok(body)
    }

    /// 发一个已编码的 batch body。成功是 **200 + 空 body**。
    async fn send_batch_raw(&self, body: Vec<u8>) -> Result<(), String> {
        let res = self
            .http_client
            .post(self.batch_url())
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("send: {e}"))?;

        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if status.is_success() {
            // 文档：成功返回空 body；这里不依赖 body 内容。
            Ok(())
        } else {
            // 400 framing/大小/tip 不足、401 key 错、429 限速、500 内部错误（纯文本）
            Err(format!("HTTP {status}: {}", text.trim()))
        }
    }

    /// 走 Batch Send 发一笔（单笔也走这条，文档说它对单笔也是最快的）。
    async fn try_batch_send(&self, tx: &VersionedTransaction) -> Result<(), String> {
        let wire = serialize_transaction_wire(tx)?;
        let body = Self::encode_batch(&[wire.as_slice()])?;
        self.send_batch_raw(body).await
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for Temporal {
    /// 优先走 Batch Send（文档说它对单笔也是最快）；非 2xx 则回落一次 JSON-RPC。
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("temporal send: ", {
            if !Self::force_json_rpc() {
                match self.try_batch_send(tx).await {
                    Ok(()) => {
                        info!("[temporal/batch] 已受理 sig={}", tx.sig());
                        return Ok(());
                    }
                    // 回落一次：路径不通 / 网关不认 octet-stream 时仍能把交易发出去。
                    // 注意 Batch Send 是流式处理，理论上可能已部分受理；同一签名重复
                    // 上链只会 AlreadyProcessed，不会重复成交。
                    Err(e) => warn!("[temporal/batch] 失败({e})，回落 JSON-RPC"),
                }
            }
            self.send_json_rpc(tx).await
        })
    }
}

impl Temporal {
    /// 老路径：JSON-RPC `sendTransaction` + base64（会带落块签名，便于兼容排查）。
    async fn send_json_rpc(&self, tx: &VersionedTransaction) -> Result<(), String> {
        let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
        let mut url = String::with_capacity(self.endpoint.len() + self.token.len() + 20);
        url.push_str(&self.endpoint);
        url.push_str("?c=");
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
        info!("temporal: {}", response);
        Ok(())
    }
}

impl crate::platform_clients::BuildTx for Temporal {
    fn get_tip_address(&self) -> Pubkey {
        *TEMPORAL_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| TEMPORAL_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Temporal
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        TEMPORAL_TIP_ACCOUNTS.to_vec()
    }

    // 使用默认实现，无需重写 build_tx
}

use std::fmt;
impl fmt::Display for Temporal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Temporal")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wire 格式：`[len_hi][len_lo][tx_bytes]...`，u16 **大端**。
    #[test]
    fn batch_wire_format_is_u16_be_prefixed() {
        let a = vec![0xaa; MIN_TX_SIZE];
        let b = vec![0xbb; MIN_TX_SIZE + 1];
        let body = Temporal::encode_batch(&[a.as_slice(), b.as_slice()]).unwrap();

        assert_eq!(&body[0..2], &(MIN_TX_SIZE as u16).to_be_bytes(), "第一笔长度前缀");
        assert_eq!(&body[2..2 + MIN_TX_SIZE], a.as_slice());
        let off = 2 + MIN_TX_SIZE;
        assert_eq!(&body[off..off + 2], &((MIN_TX_SIZE + 1) as u16).to_be_bytes());
        assert_eq!(&body[off + 2..], b.as_slice());
        // 没有任何 JSON / 分隔符：总长 = Σ(2 + len)
        assert_eq!(body.len(), 2 + a.len() + 2 + b.len());
    }

    /// 空 batch 直接拒（服务端也不会接受，别浪费一次请求）。
    #[test]
    fn empty_batch_rejected() {
        assert!(Temporal::encode_batch(&[]).is_err());
    }

    /// 笔数上限 16。
    #[test]
    fn batch_count_limit() {
        let tx = vec![0x11u8; MIN_TX_SIZE];
        let ok: Vec<&[u8]> = (0..MAX_BATCH_TXS).map(|_| tx.as_slice()).collect();
        assert!(Temporal::encode_batch(&ok).is_ok());

        let too_many: Vec<&[u8]> = (0..MAX_BATCH_TXS + 1).map(|_| tx.as_slice()).collect();
        assert!(Temporal::encode_batch(&too_many).is_err());
    }

    /// 单笔大小按**版本**判定：legacy/v0 → 1232，v1（首字节 0x81）→ 4096。
    #[test]
    fn per_version_size_limit() {
        // legacy/v0：1232 可以，1233 不行
        let ok = vec![0x00u8; MAX_TX_SIZE_LEGACY];
        assert!(Temporal::encode_batch(&[ok.as_slice()]).is_ok());
        let bad = vec![0x00u8; MAX_TX_SIZE_LEGACY + 1];
        assert!(Temporal::encode_batch(&[bad.as_slice()]).is_err());

        // v1：判别字节 0x81，4096 可以，4097 不行
        let mut v1 = vec![0x00u8; MAX_TX_SIZE_V1];
        v1[0] = V1_DISCRIMINATOR;
        assert_eq!(Temporal::max_tx_size_for(&v1), MAX_TX_SIZE_V1);
        assert!(Temporal::encode_batch(&[v1.as_slice()]).is_ok());

        let mut v1_bad = vec![0x00u8; MAX_TX_SIZE_V1 + 1];
        v1_bad[0] = V1_DISCRIMINATOR;
        assert!(Temporal::encode_batch(&[v1_bad.as_slice()]).is_err());
    }

    /// 单笔下限 66 字节。
    #[test]
    fn min_tx_size_enforced() {
        let small = vec![0u8; MIN_TX_SIZE - 1];
        assert!(Temporal::encode_batch(&[small.as_slice()]).is_err());
        let edge = vec![0u8; MIN_TX_SIZE];
        assert!(Temporal::encode_batch(&[edge.as_slice()]).is_ok());
    }

    /// body 总上限 65,568：16 笔 4096 的 v1 会超，必须拒。
    #[test]
    fn body_size_limit() {
        let mut v1 = vec![0u8; MAX_TX_SIZE_V1];
        v1[0] = V1_DISCRIMINATOR;
        let full: Vec<&[u8]> = (0..MAX_BATCH_TXS).map(|_| v1.as_slice()).collect();
        // 16 * (2 + 4096) = 65_568 —— 正好等于上限，允许
        assert_eq!(16 * (2 + MAX_TX_SIZE_V1), MAX_BATCH_BODY);
        assert!(Temporal::encode_batch(&full).is_ok());
    }

    /// Batch Send 的 URL 不能出现 `//api`（端点常量自带结尾 `/`）。
    #[test]
    fn batch_url_is_well_formed() {
        let c = Temporal::init_with("KEY", Region::Frankfurt);
        let url = c.batch_url();
        assert_eq!(url, "http://fra2.nozomi.temporal.xyz/api/sendBatch?c=KEY");
        assert!(!url.contains("//api"), "{url}");
    }

    /// 每个区域都映射到官方直连端点（QUIC/HTTP 共用同一批机房）。
    #[test]
    fn endpoints_are_direct_nozomi_hosts() {
        for ep in TEMPORAL_ENDPOINT {
            assert!(ep.starts_with("http://"), "{ep}");
            assert!(ep.contains("nozomi.temporal.xyz"), "{ep}");
            assert!(ep.ends_with('/'), "{ep}");
        }
    }
}
