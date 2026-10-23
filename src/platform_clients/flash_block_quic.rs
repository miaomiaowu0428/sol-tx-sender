//! **FlashBlock QUIC** 发送器（FLQ1 v1：单向流 + 帧格式 + **没有任何响应**）。
//!
//! 协议来源：FlashBlock《⚡ QUIC Submit》官方文档（FLQ1 v1，2026-07-28 版）。
//!
//! ```text
//! connect (UDP/6888, TLS1.3, ALPN="flash-tx/1")
//!   │
//!   ├─ 第 1 个 client uni stream：AUTH 帧（payload = API key 的 UTF-8 原始字节）
//!   │     服务端不回应；认证失败会直接关连接
//!   │
//!   └─ 之后每笔交易一个 client uni stream：SUBMIT 帧（payload = 已签名的 wire 交易）
//!         服务端不回应；flags≠0 / 非法 / 超长 / 被限速的帧被**静默丢弃**
//! ```
//!
//! 帧格式（一帧一个 stream，必须 FIN 结束）：
//!
//! ```text
//! magic[4]="FLQ1"  version[1]=1  type[1]=0x01 AUTH|0x02 SUBMIT
//! flags[1]=0       payload_len[2]=u16 BE        payload[payload_len]
//! ```
//!
//! # ⚠️ 没有响应，`Ok` 不等于上链
//!
//! 这是本仓库所有通道里**唯一没有应用层回执**的。`send_tx` 返回 `Ok(())` 只代表
//! `send.stopped()` 拿到 `None`（QUIC 确认了字节 + FIN，且对端没发 `STOP_SENDING`），
//! 它**不**代表：认证通过、帧通过校验、进入 dispatcher、到达 validator、上链成功。
//! 真正的结果只能靠签名去查（RPC / FlashBlock 的交易错误接口）。上层 confirm 逻辑
//! 本来就在等签名结果，所以不影响现有流程，但别拿 `Ok` 当"发出去了"。
//!
//! # 与 HTTP 通道的关系
//!
//! 共用同一个 API key、同一套权限/吊销状态、**同一个 TPS 限额**（QUIC 没有独立配额）。
//! 所以两条通道同时开会互相抢 TPS。
//!
//! # TLS
//!
//! 官方示例用公网根证书校验（`webpki-roots`），这里默认也做**真校验**，并且必须用
//! FQDN 连接（TLS SNI + 证书校验都依赖域名，不能用裸 IP）。调试期如需跳过校验，
//! 设 `FLASHBLOCK_QUIC_INSECURE=1`（会打印 warn）。

use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use log::{info, warn};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, Endpoint, TransportConfig};
use rand::seq::IndexedRandom;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;
use solana_tls_utils::SkipServerVerification;
use utils::log_time;

use crate::constants::REGION;
use crate::platform_clients::flash_block::{FLASH_BLOCK_TIP_ACCOUNTS, FlashBlock};
use crate::platform_clients::{PlatformName, Region, SendTx, serialize_transaction_wire};

/// FLQ1 的 QUIC 端口。
pub const FLASHBLOCK_QUIC_PORT: u16 = 6888;
/// TLS ALPN（官方文档明确要求）。
pub const FLASHBLOCK_QUIC_ALPN: &[u8] = b"flash-tx/1";

/// 帧头 magic。
const MAGIC: &[u8; 4] = b"FLQ1";
/// 协议版本（FLQ1 v1）。
const VERSION: u8 = 1;
/// 帧类型：认证。
const TYPE_AUTH: u8 = 0x01;
/// 帧类型：提交交易。
const TYPE_SUBMIT: u8 = 0x02;
/// v1 要求 flags 必须为 0；非 0 会被静默丢弃。
const FLAGS: u8 = 0;
/// 帧头长度：4 + 1 + 1 + 1 + 2。
pub const FRAME_HEADER_LEN: usize = 9;

/// 单笔交易上限（文档：1 ~ 4096 字节）。
pub const MAX_TX_SIZE: usize = 4096;
/// 文档示例里额外按 1232 卡了一道（Legacy/V0 上限）；超过只告警不拒绝，
/// 因为我们的 V1 交易可以合法超过 1232。
pub const LEGACY_TX_SIZE: usize = 1232;

/// 建立连接超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 认证帧 FIN 确认超时。
const AUTH_TIMEOUT: Duration = Duration::from_secs(3);
/// 单笔 SUBMIT 帧 FIN 确认超时。
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(3);
/// 保活间隔（文档没有给值，沿用本仓库其它 QUIC 通道的取值）。
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
/// 连接空闲超时。
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 连接并完成一次 AUTH 后的 FlashBlock QUIC 客户端。
///
/// `Clone` 只是 Arc 的引用计数克隆，**不会**新建连接或重新认证。
#[derive(Clone)]
pub struct FlashBlockQuic {
    inner: Arc<Inner>,
}

struct Inner {
    /// 必须持有 `Endpoint`：drop 掉连接会一起失效，重连也要用它。
    endpoint: Endpoint,
    /// 当前已 AUTH 的长连接。读多写极少 → `RwLock`。
    connection: RwLock<Connection>,
    /// 重连单飞锁：连接坏掉时只让一个 task 去重连 + 重新 AUTH。
    reconnect: tokio::sync::Mutex<()>,
    /// FQDN（TLS SNI 用它，日志也用它）。
    host: String,
    /// AUTH 帧的 payload：API key 的原始 UTF-8 字节。
    api_key: Vec<u8>,
}

impl FlashBlockQuic {
    /// 单笔交易最低 tip（与 HTTP 通道 [`FlashBlock`] 一致）。
    pub const MIN_TIP_AMOUNT_TX: u64 = FlashBlock::MIN_TIP_AMOUNT_TX;

    /// QUIC 用的 FQDN（不含端口）。
    ///
    /// 默认沿用 HTTP 通道同一套区域映射（`FlashBlock::endpoint_for`，去掉 scheme）；
    /// 官方要求「使用**你账号分配到的** QUIC 区域 FQDN，并先确认该区域 QUIC 可用」，
    /// 所以支持 `FLASHBLOCK_QUIC_HOST` 覆盖（可含 scheme/端口，会被规范化掉）。
    pub fn host_for(region: Region) -> String {
        if let Ok(raw) = std::env::var("FLASHBLOCK_QUIC_HOST") {
            let raw = raw.trim();
            if !raw.is_empty() {
                return normalize_host(raw);
            }
        }
        normalize_host(FlashBlock::endpoint_for(region))
    }

    /// 从 `http://host[:port]` / `host:port` 里取出裸域名。
    pub fn host_of(endpoint: &str) -> String {
        normalize_host(endpoint)
    }

    pub fn get_host() -> String {
        Self::host_for(*REGION)
    }

    /// 从环境变量 `FLASHBLOCK_KEY` + `REGION` 构造（与其它平台的 `new()` 一致）。
    pub async fn new() -> Result<Self, String> {
        let api_key = std::env::var("FLASHBLOCK_KEY").map_err(|e| format!("FLASHBLOCK_KEY env var required: {e}"))?;
        Self::init_with(api_key, *REGION).await
    }

    /// 显式构造：连接 + AUTH 一次，之后复用这条连接（断了会自动重连 + 重新 AUTH）。
    pub async fn init_with(api_key: impl Into<String>, region: Region) -> Result<Self, String> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err("FlashBlock QUIC: api key 为空".to_string());
        }

        let host = Self::host_for(region);
        let api_key = api_key.trim().as_bytes().to_vec();

        let (endpoint, connection) = tokio::time::timeout(CONNECT_TIMEOUT, Self::connect(&host))
            .await
            .map_err(|_| format!("connect timeout after {CONNECT_TIMEOUT:?} ({host}:{FLASHBLOCK_QUIC_PORT})"))??;

        tokio::time::timeout(AUTH_TIMEOUT, send_frame(&connection, TYPE_AUTH, &api_key))
            .await
            .map_err(|_| format!("auth timeout after {AUTH_TIMEOUT:?}"))??;

        info!(
            "[FlashBlockQuic] 已连接并发送 AUTH {}:{}（服务端无回执）",
            host, FLASHBLOCK_QUIC_PORT
        );

        Ok(Self {
            inner: Arc::new(Inner {
                endpoint,
                connection: RwLock::new(connection),
                reconnect: tokio::sync::Mutex::new(()),
                host,
                api_key,
            }),
        })
    }

    /// 建一条 QUIC 连接（不做认证），并把 client config 设为 endpoint 默认值，
    /// 供后续重连复用。
    async fn connect(host: &str) -> Result<(Endpoint, Connection), String> {
        let client_config = build_client_config()?;

        let mut endpoint =
            Endpoint::client("0.0.0.0:0".parse().map_err(|_| "parse bind addr 0.0.0.0:0")?).map_err(|e| e.to_string())?;
        endpoint.set_default_client_config(client_config);

        let connection = Self::handshake(&endpoint, host).await?;
        Ok((endpoint, connection))
    }

    /// 在已有 endpoint 上握手（首次连接和重连共用）。
    async fn handshake(endpoint: &Endpoint, host: &str) -> Result<Connection, String> {
        // 必须用 FQDN：TLS SNI + 证书校验都依赖它，裸 IP 会失败。
        let addr = tokio::net::lookup_host((host, FLASHBLOCK_QUIC_PORT))
            .await
            .map_err(|e| format!("resolve {host}:{FLASHBLOCK_QUIC_PORT}: {e}"))?
            .next()
            .ok_or_else(|| format!("resolve {host}:{FLASHBLOCK_QUIC_PORT}: no address"))?;

        endpoint
            .connect(addr, host)
            .map_err(|e| format!("start handshake: {e}"))?
            .await
            .map_err(|e| format!("finish handshake: {e}"))
    }

    /// 取当前可用连接；已断开则（单飞地）重连 + 重新 AUTH。
    async fn ensure_connected(&self) -> Result<Connection, String> {
        let current = self.load_connection();
        if current.close_reason().is_none() {
            return Ok(current);
        }

        let _guard = self.inner.reconnect.lock().await;
        let current = self.load_connection();
        if current.close_reason().is_none() {
            return Ok(current);
        }

        self.reconnect_now().await
    }

    fn load_connection(&self) -> Connection {
        self.inner
            .connection
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 真正重连：握手 + AUTH + 写回。调用方需持有重连锁。
    async fn reconnect_now(&self) -> Result<Connection, String> {
        warn!(
            "[FlashBlockQuic] 连接已断开({})，重连 {}",
            self.close_reason().unwrap_or_else(|| "unknown".into()),
            self.inner.host
        );

        let connection = tokio::time::timeout(
            CONNECT_TIMEOUT,
            Self::handshake(&self.inner.endpoint, &self.inner.host),
        )
        .await
        .map_err(|_| format!("reconnect timeout after {CONNECT_TIMEOUT:?}"))??;

        tokio::time::timeout(AUTH_TIMEOUT, send_frame(&connection, TYPE_AUTH, &self.inner.api_key))
            .await
            .map_err(|_| format!("re-auth timeout after {AUTH_TIMEOUT:?}"))??;

        *self.inner.connection.write().unwrap_or_else(|e| e.into_inner()) = connection.clone();
        info!("[FlashBlockQuic] 重连并重新 AUTH 成功 {}", self.inner.host);
        Ok(connection)
    }

    /// 提交一笔**已签名**的交易（wire 字节）。返回 `Ok(())` 仅表示传输层确认。
    ///
    /// 连接级失败（连接已断 / `open_uni` / `write` / `finish` 报错）会重连后重试一次；
    /// 对端 `STOP_SENDING`、超时等情况不重试（要么被明确拒绝，要么状态未知）。
    pub async fn submit_transaction(&self, wire_tx: &[u8]) -> Result<(), String> {
        if wire_tx.is_empty() {
            return Err("FlashBlock QUIC: 交易为空".to_string());
        }
        if wire_tx.len() > MAX_TX_SIZE {
            return Err(format!(
                "FlashBlock QUIC: 交易 {} 字节超过上限 {MAX_TX_SIZE}",
                wire_tx.len()
            ));
        }
        if wire_tx.len() > LEGACY_TX_SIZE {
            // 文档示例按 1232 卡；我们发 V1 时可能合法超过它，只告警。
            warn!(
                "[FlashBlockQuic] 交易 {} 字节 > {LEGACY_TX_SIZE}（Legacy/V0 上限），确认服务端接受该尺寸",
                wire_tx.len()
            );
        }

        let connection = self.ensure_connected().await?;

        match self.try_submit(&connection, wire_tx).await {
            Ok(()) => Ok(()),
            Err(SendErr { msg, retryable }) if retryable => {
                warn!("[FlashBlockQuic] 提交失败({msg})，重连后重试一次");
                let connection = self.reconnect_now().await?;
                self.try_submit(&connection, wire_tx)
                    .await
                    .map_err(|e| format!("{}（重试后）", e.msg))
            }
            Err(e) => Err(e.msg),
        }
    }

    /// 单次提交：open_uni → write(frame) → finish → 等 FIN 确认。
    async fn try_submit(&self, connection: &Connection, wire_tx: &[u8]) -> Result<(), SendErr> {
        let fut = send_frame(connection, TYPE_SUBMIT, wire_tx);
        tokio::time::timeout(SUBMIT_TIMEOUT, fut)
            .await
            .map_err(|_| SendErr::fatal(format!("submit timeout after {SUBMIT_TIMEOUT:?}（状态未知）")))?
            .map_err(SendErr::from_string)
    }

    /// 连接是否已经不可用（用于观测/告警；发送路径内部会自行处理重连）。
    pub fn close_reason(&self) -> Option<String> {
        self.load_connection().close_reason().map(|r| format!("{r:?}"))
    }

    pub fn host(&self) -> &str {
        &self.inner.host
    }
}

/// 构造 rustls/quinn 的 client config（含 ALPN）。
fn build_client_config() -> Result<quinn::ClientConfig, String> {
    let insecure = matches!(
        std::env::var("FLASHBLOCK_QUIC_INSECURE").as_deref(),
        Ok("1") | Ok("true")
    );
    if insecure {
        warn!("[FlashBlockQuic] FLASHBLOCK_QUIC_INSECURE 已开启：跳过 TLS 证书校验（仅调试用）");
    }

    let builder = rustls::ClientConfig::builder();
    let mut tls = if insecure {
        builder
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth()
    } else {
        // 官方示例用公网根证书；webpki-roots 是编译期内嵌的，不依赖宿主机证书文件。
        let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        builder.with_root_certificates(roots).with_no_client_auth()
    };

    // ALPN 是硬要求：没有 flash-tx/1 服务端不会把连接当 FLQ1。
    tls.alpn_protocols = vec![FLASHBLOCK_QUIC_ALPN.to_vec()];

    let client_crypto = QuicClientConfig::try_from(tls).map_err(|e| format!("build quic tls config: {e}"))?;
    let mut client_config = quinn::ClientConfig::new(Arc::new(client_crypto));

    let mut transport = TransportConfig::default();
    transport.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(MAX_IDLE_TIMEOUT).map_err(|e| format!("idle timeout: {e}"))?,
    ));
    transport.keep_alive_interval(Some(KEEP_ALIVE_INTERVAL));
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}

/// 编码一个 FLQ1 帧。
///
/// `payload_len` 是 u16 BE，所以 payload 本身不能超过 65535；交易另有 4096 上限，
/// 由调用方检查。
pub fn encode_frame(kind: u8, payload: &[u8]) -> Result<Vec<u8>, String> {
    let payload_len = u16::try_from(payload.len()).map_err(|_| format!("payload {} 字节超过 u16 上限", payload.len()))?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.push(VERSION);
    frame.push(kind);
    frame.push(FLAGS);
    frame.extend_from_slice(&payload_len.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// 在一个新的 client uni stream 上发一帧，并等 FIN 确认。
///
/// `stopped() == None` = 对端确认了字节 + FIN 且没发 `STOP_SENDING`。
/// ⚠️ 这是**传输层**确认，不是应用层受理。
async fn send_frame(connection: &Connection, kind: u8, payload: &[u8]) -> Result<(), String> {
    let frame = encode_frame(kind, payload)?;

    let mut send = connection
        .open_uni()
        .await
        .map_err(|e| format!("open_uni: {e}"))?;

    send.write_all(&frame)
        .await
        .map_err(|e| format!("write: {e}"))?;
    // FIN 必须：服务端靠 stream EOF 判断一帧结束。
    send.finish().map_err(|e| format!("finish: {e}"))?;

    match send.stopped().await {
        Ok(None) => Ok(()),
        // 对端 STOP_SENDING：帧被明确拒绝（认证失败 / 限速 / 格式错）。
        Ok(Some(code)) => Err(format!("peer stopped stream: code={code:?}")),
        Err(e) => Err(format!("stopped: {e}")),
    }
}

/// 单次提交的错误；`retryable` 表示「重连后重试是安全的」。
struct SendErr {
    msg: String,
    retryable: bool,
}

impl SendErr {
    fn fatal(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            retryable: false,
        }
    }

    /// `send_frame` 的错误统一在这里归类：
    ///
    /// - `open_uni` / `write` / `finish` 失败 → 连接级问题，重连可重试；
    /// - `peer stopped stream` / `stopped:` → 对端明确拒绝或状态未知，不重试。
    fn from_string(msg: String) -> Self {
        let retryable = msg.starts_with("open_uni:")
            || msg.starts_with("write:")
            || msg.starts_with("finish:");
        Self { msg, retryable }
    }
}

/// 规范化成裸域名：去掉 scheme、路径、端口、首尾空白。
fn normalize_host(raw: &str) -> String {
    let s = raw.trim();
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(s);
    let s = s.split('/').next().unwrap_or(s);
    // IPv6 字面量形如 [::1]:6888，这里只处理常见 host[:port]
    match s.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !host.contains(':') => host.to_string(),
        _ => s.to_string(),
    }
}

impl fmt::Display for FlashBlockQuic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FlashBlockQuic({}:{})", self.inner.host, FLASHBLOCK_QUIC_PORT)
    }
}

#[async_trait::async_trait]
impl SendTx for FlashBlockQuic {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("flashblock quic send: ", {
            // 必须发 wire 字节（不是 bincode、不是 base64）。
            let bytes = serialize_transaction_wire(tx)?;
            self.submit_transaction(&bytes).await?;
            // ⚠️ 到这只说明 FIN 被传输层确认，不代表受理/上链。
            info!(
                "[FlashBlockQuic] {} 已提交（传输层确认，无应用回执）sig={}",
                self.inner.host,
                tx.signatures.first().map(|s| s.to_string()).unwrap_or_default()
            );
            Ok(())
        })
    }
}

impl crate::platform_clients::BuildTx for FlashBlockQuic {
    fn platform(&self) -> PlatformName {
        // 与 HTTP 通道共用同一平台语义（tip / 最低 tip / 计价方式一致）。
        PlatformName::FlashBlock
    }

    fn get_tip_address(&self) -> Pubkey {
        *FLASH_BLOCK_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| FLASH_BLOCK_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        FLASH_BLOCK_TIP_ACCOUNTS.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 帧头布局必须与文档逐字节一致（9 字节，u16 BE 长度）。
    #[test]
    fn frame_layout_matches_spec() {
        let frame = encode_frame(TYPE_SUBMIT, &[0xaa, 0xbb, 0xcc]).unwrap();
        assert_eq!(&frame[0..4], b"FLQ1", "magic");
        assert_eq!(frame[4], 1, "version 必须是 1");
        assert_eq!(frame[5], TYPE_SUBMIT, "type");
        assert_eq!(frame[6], 0, "flags 必须为 0");
        assert_eq!(&frame[7..9], &[0x00, 0x03], "payload_len 必须 u16 BE");
        assert_eq!(&frame[9..], &[0xaa, 0xbb, 0xcc]);
        assert_eq!(frame.len(), FRAME_HEADER_LEN + 3);
    }

    /// AUTH 帧 payload 就是 API key 的原始字节（不做 base64、不加长度前缀以外的处理）。
    #[test]
    fn auth_frame_carries_raw_api_key_bytes() {
        let key = "my-flashblock-key";
        let frame = encode_frame(TYPE_AUTH, key.as_bytes()).unwrap();
        assert_eq!(frame[5], TYPE_AUTH);
        assert_eq!(&frame[7..9], &[0x00, key.len() as u8]);
        assert_eq!(&frame[9..], key.as_bytes());
    }

    /// payload 超 u16 必须报错，而不是静默截断（截断会被服务端丢弃且没有任何回执）。
    #[test]
    fn oversized_payload_is_rejected() {
        let big = vec![0u8; u16::MAX as usize + 1];
        assert!(encode_frame(TYPE_SUBMIT, &big).is_err());
        // 边界：u16::MAX 可以编码
        let edge = vec![0u8; u16::MAX as usize];
        assert!(encode_frame(TYPE_SUBMIT, &edge).is_ok());
    }

    /// 空帧是合法的（长度 0），由调用方决定是否允许。
    #[test]
    fn empty_payload_is_encodable() {
        let frame = encode_frame(TYPE_SUBMIT, &[]).unwrap();
        assert_eq!(frame.len(), FRAME_HEADER_LEN);
        assert_eq!(&frame[7..9], &[0, 0]);
    }

    /// host 规范化：scheme / 路径 / 端口 / 空白都要处理掉。
    #[test]
    fn host_normalization() {
        let cases = [
            ("http://ny.flashblock.trade", "ny.flashblock.trade"),
            ("https://ny.flashblock.trade", "ny.flashblock.trade"),
            ("ny.flashblock.trade:6888", "ny.flashblock.trade"),
            ("ny.flashblock.trade/", "ny.flashblock.trade"),
            ("  ny.flashblock.trade  ", "ny.flashblock.trade"),
            ("http://fra.flashblock.trade:6888/", "fra.flashblock.trade"),
        ];
        for (input, expected) in cases {
            assert_eq!(FlashBlockQuic::host_of(input), expected, "{input}");
        }
    }

    /// 区域 → host 映射沿用 HTTP 通道（同一个 `endpoint_for`），并全部落到 flashblock.trade。
    #[test]
    fn host_per_region_follows_http_channel() {
        let cases = [
            (Region::NewYork, "ny.flashblock.trade"),
            (Region::SaltLakeCity, "slc.flashblock.trade"),
            (Region::Amsterdam, "ams.flashblock.trade"),
            (Region::Frankfurt, "fra.flashblock.trade"),
            (Region::Singapore, "singapore.flashblock.trade"),
            (Region::London, "london.flashblock.trade"),
            (Region::Unknown, "fra.flashblock.trade"),
        ];
        for (region, expected) in cases {
            assert_eq!(FlashBlockQuic::host_for(region), expected, "{region:?}");
        }
    }

    /// 错误归类：连接级可重试，对端拒绝/超时不可重试。
    #[test]
    fn retry_classification() {
        assert!(SendErr::from_string("open_uni: closed".into()).retryable);
        assert!(SendErr::from_string("write: reset".into()).retryable);
        assert!(SendErr::from_string("finish: closed".into()).retryable);
        assert!(!SendErr::from_string("peer stopped stream: code=Some(1)".into()).retryable);
        assert!(!SendErr::fatal("submit timeout").retryable);
    }

    /// 上限常量必须与文档一致：交易 1~4096 字节。
    #[test]
    fn size_limits_match_spec() {
        assert_eq!(MAX_TX_SIZE, 4096);
        assert_eq!(FLASHBLOCK_QUIC_PORT, 6888);
        assert_eq!(FLASHBLOCK_QUIC_ALPN, b"flash-tx/1");
    }
}
