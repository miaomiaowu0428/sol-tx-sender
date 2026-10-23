//! **Node1 QUIC** 发送器（UUID 认证 + 双向 stream + 6 字节响应头）。
//!
//! 协议来源：Node1 官方《QUIC API》文档（2026-10 同步）。与 Astralane / EverStake
//! 的 QUIC 通道不同，Node1 是**有认证**的：
//!
//! ```text
//! connect ──► open_bi() ──► write(UUID 的 16 字节二进制) ──► finish()
//!                                                              │
//!                                                     read 1 byte: 0 = OK
//! 每笔交易：
//! open_bi() ──► write(交易 wire 字节) ──► finish() ──► read [2B status][4B msg_len][msg]
//! ```
//!
//! # 关键约定（照抄文档，改之前先看这里）
//!
//! - **认证是第一个双向 stream**，内容是 UUID 解析后的 **16 字节二进制**，不是
//!   36 字符的字符串。5 秒内没发完，服务端直接关连接。
//! - **每笔交易一个新的双向 stream**，一条 stream 只发一笔。
//! - 请求体**没有长度前缀**，服务端靠 stream EOF（`finish()`）判断一笔结束。
//!   超过 5 秒不 `finish()` → 服务端回 `408` 但**不**关连接（这笔交易丢了）。
//! - 响应帧：`status: u16 BE` + `msg_len: u32 BE` + `msg`（UTF-8）。
//!   `status == 200` 时 `msg` 是 JSON-RPC 结果；否则 `msg` 是错误信息。
//! - **认证成功后复用同一条连接**，不要每笔都 connect + auth。
//!
//! # 断线重连
//!
//! 文档明确要求：出现 `TimedOut` / `LocallyClosed` / `ApplicationClosed` /
//! `ConnectionLost(..)` 时要 reconnect + 重新 auth。长连接也一定会掉（idle timeout、
//! 服务端重启、LB 掐连接），所以这里做「读锁取连接 + 失效才抢占重连锁」的
//! double-check 单飞重连：
//!
//! - 正常发送只有一次读锁 + 一次 `close_reason()` 检查，**没有 I/O**；
//! - 连接坏了只有一个 task 真正去重连，其余 task 等它完成后直接复用新连接。
//!
//! # 序列化
//!
//! 文档示例写的是 `bincode::serialize(&versioned_tx)`。在本仓库用的 SDK 版本下，
//! **Legacy / V0 的 bincode 字节与 wire 字节完全相同**，但 **V1 不同**（bincode 会把
//! 长度前缀写成 u64，产出的不是合法 wire，见 [`crate::platform_clients::serialize_transaction_wire`]
//! 的注释和 `tests/tx_wire_format.rs`）。所以这里统一用
//! [`serialize_transaction_wire`]（wincode）：对 Legacy / V0 与文档要求等价，且额外
//! 兼容 V1。

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
use crate::platform_clients::nodeone::NODEONE_TIP_ACCOUNTS;
use crate::platform_clients::{PlatformName, Region, SendTx, serialize_transaction_wire};

/// Node1 QUIC 端口（所有区域一致）。
pub const NODEONE_QUIC_PORT: u16 = 16666;

/// 认证成功码（服务端返回的第 1 个字节）。
const AUTH_OK: u8 = 0;
/// 交易成功的状态码。
const STATUS_OK: u16 = 200;

/// 建立连接超时（文档建议：不设超时会卡死在黑洞路径上）。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 认证超时（服务端 5 秒收不到 16 字节就会关连接）。
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// 单笔发送超时（含 open_bi / write / finish / 读响应）。
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// 保活间隔（文档参考值 15s）。
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
/// 连接空闲超时（文档参考值 60s；连接级，非 stream 级）。
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 响应体长度上限：防御坏包/恶意包让我们按 u32 申请 4GB 内存。
const MAX_RESPONSE_MSG_LEN: usize = 64 * 1024;

/// 读取认证响应时最多读 8 字节（服务端只回 1 字节，多读一点容错）。
const AUTH_REPLY_BUF: usize = 8;

/// 官方 8 个区域 QUIC 端点（`host:16666`）。
///
/// 顺序与 [`NodeOneQuic::endpoint_for`] 的映射一致。
pub const NODEONE_QUIC_ENDPOINTS: &[&str] = &[
    "ny.node1.me:16666",   // New York
    "fra.node1.me:16666",  // Frankfurt
    "ams.node1.me:16666",  // Amsterdam
    "lon.node1.me:16666",  // London
    "tk.node1.me:16666",   // Tokyo
    "sgp.node1.me:16666",  // Singapore
    "fra2.node1.me:16666", // Frankfurt 2
    "dub.node1.me:16666",  // Dublin
];

/// 连接并完成一次认证后的 Node1 QUIC 客户端。
///
/// `Clone` 只是 Arc 的引用计数克隆，**不会**新建连接或重新认证。
#[derive(Clone)]
pub struct NodeOneQuic {
    inner: Arc<Inner>,
}

struct Inner {
    /// 必须持有 `Endpoint`：drop 掉连接会一起失效，重连也要用它。
    endpoint: Endpoint,
    /// 当前已认证长连接。读多写极少 → `RwLock`，正常发送只取一次读锁。
    connection: RwLock<Connection>,
    /// 重连单飞锁：连接坏掉时只让一个 task 去重连 + 重认证。
    reconnect: tokio::sync::Mutex<()>,
    /// 仅用于日志/测试。
    endpoint_addr: &'static str,
    server_name: &'static str,
    /// 重连要重新认证，缓存解析好的 16 字节（不保存/不重复解析字符串）。
    api_key_uuid: [u8; 16],
}

impl NodeOneQuic {
    /// 单笔交易最低 tip（与 HTTP 通道 [`crate::platform_clients::nodeone::NodeOne`] 一致）。
    pub const MIN_TIP_AMOUNT_TX: u64 = 100_000;

    /// 按区域返回 Node1 QUIC 接入点（纯函数）。
    ///
    /// 没有机房的区域就近回退：
    /// - `Pittsburgh` / `LosAngeles` / `SaltLakeCity` → New York
    /// - `Limburg` / `Lithuania` → Amsterdam
    /// - `Unknown` → Frankfurt
    pub fn endpoint_for(region: Region) -> &'static str {
        match region {
            Region::NewYork | Region::Pittsburgh | Region::LosAngeles | Region::SaltLakeCity => {
                NODEONE_QUIC_ENDPOINTS[0]
            }
            Region::Amsterdam | Region::Limburg | Region::Lithuania => NODEONE_QUIC_ENDPOINTS[2],
            Region::London => NODEONE_QUIC_ENDPOINTS[3],
            Region::Tokyo => NODEONE_QUIC_ENDPOINTS[4],
            Region::Singapore => NODEONE_QUIC_ENDPOINTS[5],
            Region::Frankfurt => NODEONE_QUIC_ENDPOINTS[1],
            Region::Frankfurt2 => NODEONE_QUIC_ENDPOINTS[6],
            Region::Dublin => NODEONE_QUIC_ENDPOINTS[7],
            Region::Unknown => NODEONE_QUIC_ENDPOINTS[1],
        }
    }

    pub fn get_endpoint() -> String {
        Self::endpoint_for(*REGION).to_string()
    }

    /// 从 `host:port` 取出 `server_name`（TLS SNI 必须与目标域名一致）。
    pub fn server_name_of(endpoint: &str) -> &str {
        endpoint.rsplit_once(':').map(|(host, _)| host).unwrap_or(endpoint)
    }

    /// 从环境变量 `NODEONE_KEY` + `REGION` 构造（与其它平台的 `new()` 一致）。
    pub async fn new() -> Result<Self, String> {
        let api_key = std::env::var("NODEONE_KEY").map_err(|e| format!("NODEONE_KEY env var required: {e}"))?;
        Self::init_with(api_key, *REGION).await
    }

    /// 显式构造：连接 + 认证一次，之后复用这条连接（断了会自动重连 + 重认证）。
    pub async fn init_with(api_key: impl Into<String>, region: Region) -> Result<Self, String> {
        let api_key = api_key.into();
        let endpoint_addr = Self::endpoint_for(region);
        let server_name = Self::server_name_of(endpoint_addr);

        // 先校验 key 格式：格式错就别浪费一次 QUIC 握手。
        let api_key_uuid = parse_api_key_uuid(&api_key)?;

        let (endpoint, connection) = tokio::time::timeout(CONNECT_TIMEOUT, Self::connect(endpoint_addr, server_name))
            .await
            .map_err(|_| format!("connect timeout after {CONNECT_TIMEOUT:?} ({endpoint_addr})"))??;

        tokio::time::timeout(AUTH_TIMEOUT, Self::authenticate(&connection, &api_key_uuid))
            .await
            .map_err(|_| format!("auth timeout after {AUTH_TIMEOUT:?} ({endpoint_addr})"))??;

        info!("[NodeOneQuic] 已连接并认证 {endpoint_addr}");

        Ok(Self {
            inner: Arc::new(Inner {
                endpoint,
                connection: RwLock::new(connection),
                reconnect: tokio::sync::Mutex::new(()),
                endpoint_addr,
                server_name,
                api_key_uuid,
            }),
        })
    }

    /// 建一条 QUIC 连接（不做认证）。同时把 client config 设为 endpoint 默认值，
    /// 后续 `ensure_connected` 的重连直接复用它。
    async fn connect(endpoint_addr: &str, server_name: &str) -> Result<(Endpoint, Connection), String> {
        let mut crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();

        // 文档没有约定 ALPN；万一服务端强制要求，可用 NODEONE_QUIC_ALPN 兜底，
        // 不用改代码重新发版。
        if let Ok(alpn) = std::env::var("NODEONE_QUIC_ALPN") {
            if !alpn.trim().is_empty() {
                crypto.alpn_protocols = vec![alpn.trim().as_bytes().to_vec()];
            }
        }

        let client_crypto = QuicClientConfig::try_from(crypto).map_err(|e| format!("build quic tls config: {e}"))?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(client_crypto));

        let mut transport = TransportConfig::default();
        // max_idle_timeout 是连接级：只要还在发 keepalive，这条连接就不算空闲。
        transport.max_idle_timeout(Some(
            quinn::IdleTimeout::try_from(MAX_IDLE_TIMEOUT).map_err(|e| format!("idle timeout: {e}"))?,
        ));
        transport.keep_alive_interval(Some(KEEP_ALIVE_INTERVAL));
        client_config.transport_config(Arc::new(transport));

        let mut endpoint =
            Endpoint::client("0.0.0.0:0".parse().map_err(|_| "parse bind addr 0.0.0.0:0")?).map_err(|e| e.to_string())?;
        endpoint.set_default_client_config(client_config);

        let connection = Self::handshake(&endpoint, endpoint_addr, server_name).await?;
        Ok((endpoint, connection))
    }

    /// 在已有 endpoint 上握手（首次连接和重连共用）。
    async fn handshake(endpoint: &Endpoint, endpoint_addr: &str, server_name: &str) -> Result<Connection, String> {
        // 端点写的是域名 → 用 tokio 的异步解析，别在热路径上阻塞 runtime。
        let addr = tokio::net::lookup_host(endpoint_addr)
            .await
            .map_err(|e| format!("resolve {endpoint_addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("resolve {endpoint_addr}: no address"))?;

        endpoint
            .connect(addr, server_name)
            .map_err(|e| format!("start handshake: {e}"))?
            .await
            .map_err(|e| format!("finish handshake: {e}"))
    }

    /// 第一个双向 stream 上发 16 字节 UUID 完成认证。
    async fn authenticate(connection: &Connection, api_key_uuid: &[u8; 16]) -> Result<(), String> {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|e| format!("auth open_bi: {e}"))?;

        send.write_all(api_key_uuid)
            .await
            .map_err(|e| format!("auth write: {e}"))?;
        send.finish().map_err(|e| format!("auth finish: {e}"))?;

        let mut buf = [0u8; AUTH_REPLY_BUF];
        // 用 read（有数据就返回）而不是 read_to_end：不依赖服务端是否关 stream。
        match recv.read(&mut buf).await {
            Ok(Some(n)) if n > 0 => {
                if buf[0] == AUTH_OK {
                    Ok(())
                } else {
                    Err(format!("auth rejected, reply={}", buf[0]))
                }
            }
            Ok(Some(_)) => Err("auth failed: empty reply".to_string()),
            Ok(None) => Err("auth failed: stream closed without reply".to_string()),
            Err(e) => Err(format!("auth read: {e}")),
        }
    }

    /// 取当前可用连接；已断开则（单飞地）重连 + 重新认证。
    ///
    /// 热路径只有一次读锁 + 一次 `close_reason()` 检查，没有 I/O。
    async fn ensure_connected(&self) -> Result<Connection, String> {
        let current = self.load_connection();
        if current.close_reason().is_none() {
            return Ok(current);
        }

        let _guard = self.inner.reconnect.lock().await;
        // 拿锁后复查：可能已被别的 task 重连好了。
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

    /// 真正重连：握手 + 认证 + 写回。调用方需持有重连锁。
    async fn reconnect_now(&self) -> Result<Connection, String> {
        warn!(
            "[NodeOneQuic] 连接已断开({})，重连 {}",
            self.close_reason().unwrap_or_else(|| "unknown".into()),
            self.inner.endpoint_addr
        );

        let connection = tokio::time::timeout(
            CONNECT_TIMEOUT,
            Self::handshake(&self.inner.endpoint, self.inner.endpoint_addr, self.inner.server_name),
        )
        .await
        .map_err(|_| format!("reconnect timeout after {CONNECT_TIMEOUT:?}"))??;

        tokio::time::timeout(AUTH_TIMEOUT, Self::authenticate(&connection, &self.inner.api_key_uuid))
            .await
            .map_err(|_| format!("re-auth timeout after {AUTH_TIMEOUT:?}"))??;

        *self.inner.connection.write().unwrap_or_else(|e| e.into_inner()) = connection.clone();
        info!("[NodeOneQuic] 重连并重新认证成功 {}", self.inner.endpoint_addr);
        Ok(connection)
    }

    /// 发一笔原始交易字节，返回服务端 `msg`（成功时是 JSON-RPC 结果）。
    ///
    /// 连接级失败（连接已断 / `open_bi` / `write` / `finish` 报错）会重连后重试**一次**；
    /// 已经 `finish()` 完再读响应失败的情况**不重试** —— 那种状态下无法确定服务端
    /// 是否已受理，宁可直接报错交给上层决策。
    pub async fn send_raw_transaction(&self, raw: &[u8]) -> Result<String, String> {
        let connection = self.ensure_connected().await?;

        match self.try_send(&connection, raw).await {
            Ok(msg) => Ok(msg),
            Err(SendErr { msg, retryable }) if retryable => {
                warn!("[NodeOneQuic] 发送失败({msg})，重连后重试一次");
                let connection = self.reconnect_now().await?;
                self.try_send(&connection, raw)
                    .await
                    .map_err(|e| format!("{}（重试后）", e.msg))
            }
            Err(e) => Err(e.msg),
        }
    }

    /// 单次发送：open_bi → write → finish → 读响应帧。
    async fn try_send(&self, connection: &Connection, raw: &[u8]) -> Result<String, SendErr> {
        let fut = async {
            let (mut send, mut recv) = connection
                .open_bi()
                .await
                .map_err(|e| SendErr::retryable(format!("open_bi: {e}")))?;

            send.write_all(raw)
                .await
                .map_err(|e| SendErr::retryable(format!("write: {e}")))?;
            // 必须 finish：服务端靠 stream EOF 判断一笔请求结束。
            send.finish().map_err(|e| SendErr::retryable(format!("finish: {e}")))?;

            // 到这里服务端可能已经受理，之后失败一律不重试。
            read_response(&mut recv).await.map_err(SendErr::fatal)
        };

        let (status, msg) = tokio::time::timeout(SEND_TIMEOUT, fut)
            .await
            .map_err(|_| SendErr::fatal(format!("send timeout after {SEND_TIMEOUT:?}")))??;

        if status == STATUS_OK {
            Ok(msg)
        } else {
            // 非 200 是业务错误（tip 不足 / 重复 / 格式错…），重试没意义。
            Err(SendErr::fatal(format!("status={status} msg={msg}")))
        }
    }

    /// 连接是否已经不可用（用于观测/告警；发送路径内部会自行处理重连）。
    pub fn close_reason(&self) -> Option<String> {
        self.load_connection().close_reason().map(|r| format!("{r:?}"))
    }

    pub fn endpoint_addr(&self) -> &'static str {
        self.inner.endpoint_addr
    }
}

/// 单次发送的错误；`retryable` 表示「重连后重试是安全的」。
struct SendErr {
    msg: String,
    retryable: bool,
}

impl SendErr {
    fn retryable(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            retryable: true,
        }
    }
    fn fatal(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            retryable: false,
        }
    }
}

/// 读响应帧：`status: u16 BE` + `msg_len: u32 BE` + `msg`。
async fn read_response(recv: &mut quinn::RecvStream) -> Result<(u16, String), String> {
    let mut header = [0u8; 6];
    recv.read_exact(&mut header)
        .await
        .map_err(|e| format!("read header: {e}"))?;

    let status = u16::from_be_bytes([header[0], header[1]]);
    let msg_len = u32::from_be_bytes([header[2], header[3], header[4], header[5]]) as usize;

    if msg_len > MAX_RESPONSE_MSG_LEN {
        return Err(format!("response msg_len={msg_len} 超过上限 {MAX_RESPONSE_MSG_LEN}"));
    }

    let mut msg = vec![0u8; msg_len];
    if msg_len > 0 {
        recv.read_exact(&mut msg)
            .await
            .map_err(|e| format!("read msg({msg_len}): {e}"))?;
    }

    Ok((status, String::from_utf8_lossy(&msg).into_owned()))
}

/// 把 API key（UUID 字符串）解析成 16 字节二进制。
///
/// 支持带 `-` 的标准形（8-4-4-4-12）和无 `-` 的 32 位十六进制；大小写均可。
/// 认证要的正是这 16 字节，**不是**字符串本身。
pub fn parse_api_key_uuid(api_key: &str) -> Result<[u8; 16], String> {
    let raw = api_key.trim();
    let hex: Vec<u8> = raw.bytes().filter(|b| *b != b'-').collect();

    if hex.len() != 32 {
        return Err(format!(
            "api key 不是合法 UUID：去掉 '-' 后应为 32 位十六进制，实际 {} 位",
            hex.len()
        ));
    }

    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = hex_to_nibble(hex[i * 2]).ok_or_else(|| format!("api key 第 {} 字节高 4 位不是十六进制", i))?;
        let lo = hex_to_nibble(hex[i * 2 + 1]).ok_or_else(|| format!("api key 第 {} 字节低 4 位不是十六进制", i))?;
        *byte = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_to_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for NodeOneQuic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeOneQuic({})", self.inner.endpoint_addr)
    }
}

#[async_trait::async_trait]
impl SendTx for NodeOneQuic {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("node1 quic send: ", {
            // 统一走 wire 序列化：Legacy/V0 与文档的 bincode 等价，V1 只有它有正确字节。
            let bytes = serialize_transaction_wire(tx)?;
            match self.send_raw_transaction(&bytes).await {
                Ok(msg) => {
                    info!("[NodeOneQuic] {} 上链请求已受理: {}", self.inner.endpoint_addr, msg);
                    Ok(())
                }
                Err(e) => Err(format!("NodeOneQuic send error: {e}")),
            }
        })
    }
}

impl crate::platform_clients::BuildTx for NodeOneQuic {
    fn platform(&self) -> PlatformName {
        // 与 HTTP 通道共用同一平台语义（tip / 最低 tip / 计价方式完全一致）。
        PlatformName::Nodeone
    }

    fn get_tip_address(&self) -> Pubkey {
        *NODEONE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| NODEONE_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        NODEONE_TIP_ACCOUNTS.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 8 个区域端点，全部带 :16666 且互不重复。
    #[test]
    fn endpoints_are_complete_and_unique() {
        assert_eq!(NODEONE_QUIC_ENDPOINTS.len(), 8);
        let mut uniq = NODEONE_QUIC_ENDPOINTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 8, "NODEONE_QUIC_ENDPOINTS 有重复");
        for ep in NODEONE_QUIC_ENDPOINTS {
            assert!(ep.ends_with(":16666"), "{ep}");
            // server_name 必须是裸域名
            let host = NodeOneQuic::server_name_of(ep);
            assert!(!host.contains(':'), "{ep}");
            assert!(host.ends_with(".node1.me"), "{ep}");
            assert_eq!(NodeOneQuic::server_name_of(host), host, "裸域名应原样返回");
        }
    }

    /// 8 个机房一一对应，其余就近回退。
    #[test]
    fn endpoint_per_region() {
        let cases = [
            (Region::NewYork, "ny.node1.me:16666"),
            (Region::Frankfurt, "fra.node1.me:16666"),
            (Region::Amsterdam, "ams.node1.me:16666"),
            (Region::London, "lon.node1.me:16666"),
            (Region::Tokyo, "tk.node1.me:16666"),
            (Region::Singapore, "sgp.node1.me:16666"),
            (Region::Frankfurt2, "fra2.node1.me:16666"),
            (Region::Dublin, "dub.node1.me:16666"),
            // 就近回退
            (Region::LosAngeles, "ny.node1.me:16666"),
            (Region::SaltLakeCity, "ny.node1.me:16666"),
            (Region::Pittsburgh, "ny.node1.me:16666"),
            (Region::Limburg, "ams.node1.me:16666"),
            (Region::Lithuania, "ams.node1.me:16666"),
            (Region::Unknown, "fra.node1.me:16666"),
        ];
        for (region, expected) in cases {
            assert_eq!(NodeOneQuic::endpoint_for(region), expected, "{region:?}");
            assert!(NODEONE_QUIC_ENDPOINTS.contains(&expected));
        }
    }

    /// UUID 解析：带 `-` / 不带 `-` / 大小写都要等价。
    #[test]
    fn parse_uuid_accepts_common_forms() {
        let dashed = "550e8400-e29b-41d4-a716-446655440000";
        let expected = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00, 0x00,
        ];
        assert_eq!(parse_api_key_uuid(dashed).unwrap(), expected);
        assert_eq!(
            parse_api_key_uuid("550E8400E29B41D4A716446655440000").unwrap(),
            expected
        );
        assert_eq!(
            parse_api_key_uuid("  550e8400-e29b-41d4-a716-446655440000  ").unwrap(),
            expected
        );
    }

    /// 非法 key 必须被拦在握手之前（省一次 QUIC 握手 + 一次日志排查）。
    #[test]
    fn parse_uuid_rejects_bad_input() {
        assert!(parse_api_key_uuid("").is_err());
        assert!(parse_api_key_uuid("not-a-uuid").is_err());
        // 少一位
        assert!(parse_api_key_uuid("550e8400-e29b-41d4-a716-44665544000").is_err());
        // 多一位
        assert!(parse_api_key_uuid("550e8400-e29b-41d4-a716-4466554400000").is_err());
        // 含非十六进制字符
        assert!(parse_api_key_uuid("550e8400-e29b-41d4-a716-44665544zzzz").is_err());
    }

    /// 响应帧解析的字节序假设（大端），防止有人改成小端。
    #[test]
    fn response_header_is_big_endian() {
        let status: u16 = 200;
        let msg_len: u32 = 5;
        let mut frame = Vec::new();
        frame.extend_from_slice(&status.to_be_bytes());
        frame.extend_from_slice(&msg_len.to_be_bytes());
        frame.extend_from_slice(b"hello");

        assert_eq!(&frame[0..2], &[0x00, 0xC8], "status 必须大端 2 字节");
        assert_eq!(&frame[2..6], &[0x00, 0x00, 0x00, 0x05], "msg_len 必须大端 4 字节");
        assert_eq!(frame.len(), 6 + 5);
    }

    /// 重试语义：只有「还没写完这笔」的失败才可重试。
    #[test]
    fn retryable_flags_are_right() {
        assert!(SendErr::retryable("open_bi").retryable);
        assert!(SendErr::retryable("write").retryable);
        assert!(SendErr::retryable("finish").retryable);
        // 读到响应之后的状态一律不重试（服务端可能已受理）
        assert!(!SendErr::fatal("status=409").retryable);
        assert!(!SendErr::fatal("send timeout").retryable);
    }
}
