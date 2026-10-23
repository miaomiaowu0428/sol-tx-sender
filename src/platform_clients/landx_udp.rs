//! **LandX UDP Binary-Tx** 发送器（`UDP <host>:10000`）。
//!
//! 协议来源：LandX 官方文档「UDP Binary-Tx」。包体就两段，**没有帧头、没有长度前缀**：
//!
//! ```text
//! [ 12 字节 API key ][ 原始 Solana wire 交易字节 ... ]
//! ```
//!
//! # 三个必须知道的事实
//!
//! 1. **UDP 没有响应**。`sendto` 返回 `Ok` 只代表「内核收下了这个包」，
//!    不代表到达、不代表认证通过、不代表上链。结果只能拿签名去查。
//! 2. **API key 必须是恰好 12 字节**（直接拼在包里，没有分隔符，长度错了服务端
//!    只会静默丢弃）。所以这里在构造时就校验，不合格直接拒绝启动这个通道。
//! 3. **超过 MTU 会分片**。一个 tx 最多 1232（legacy/v0）或 4096（v1）字节，
//!    加上 12 字节 key 后，v1 交易必然超过 1472（1500 MTU - 20 - 8）→ IP 分片，
//!    **丢一个分片整包作废**。这里不会拒绝，但会 warn 一次（见 `MTU_WARN_BYTES`）。
//!
//! # 相比 HTTP 通道多出的机房
//!
//! [`crate::platform_clients::landx::LANDX_UDP_ONLY_HOSTS`] 里的
//! `fra2` / `fra3` / `slc1` / `dub1` **只有 UDP**，HTTP 客户端当时只能把这些区域
//! 回退到 `fra1` / `ny1` / `lon1`。本客户端把它们真正用起来。
//!
//! # 为什么 UDP 可能更快（也解释了为什么不一定）
//!
//! 省掉的是：TCP 握手 / TLS 记录层加解密 / TCP 的 ACK 与重传状态机。
//! - **冷连接**：省掉 TCP+TLS 建立（1~2 RTT + 证书校验），收益明显。
//! - **热连接**：`send()` 和 `sendto()` 都是「交给内核就返回」，用户态感知的延迟
//!   差别很小；真正的差别在**丢包路径**——TCP 会重传并把后续包排在同一连接里
//!   （队头阻塞），UDP 丢一个包只丢那一笔。
//! - **服务端 ingress 更早拿到交易**：少了解密 + HTTP 解析，这一段是实打实的。
//!
//! 代价：没有任何可靠性保证，也没有回执。所以它适合「同一笔交易同时走多个通道」
//! 的场景（我们本来就是多平台齐发），而不是替代 HTTP。

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{info, warn};
use rand::seq::IndexedRandom;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;
use tokio::net::UdpSocket;
use utils::log_time;

use crate::constants::REGION;
use crate::platform_clients::landx::{LANDX_TIP_ACCOUNTS, LandX};
use crate::platform_clients::{PlatformName, Region, SendTx, serialize_transaction_wire};

/// LandX UDP 端口。
pub const LANDX_UDP_PORT: u16 = 10000;

/// API key 在包体里占的字节数（固定 12，没有长度前缀）。
pub const LANDX_API_KEY_LEN: usize = 12;

/// IPv4 UDP 单个数据报的理论上限（65535 - 20 IP - 8 UDP）。
pub const MAX_UDP_PAYLOAD: usize = 65_507;

/// 不分片的单包上限（1500 MTU - 20 IP - 8 UDP）。
///
/// 包体超过这个值就会被 IP 分片，任意一个分片丢失都会让整包作废。
pub const MTU_WARN_BYTES: usize = 1472;

/// LandX **全部** UDP 机房（含仅 UDP 的 4 个），按区域就近选择。
pub const LANDX_UDP_HOSTS: &[&str] = &[
    "ams1.landx.dev", // Amsterdam (TeraSwitch)
    "ams2.landx.dev", // Amsterdam2 (Cherry Servers)
    "fra1.landx.dev", // Frankfurt1 (TeraSwitch)
    "fra2.landx.dev", // Frankfurt2（仅 UDP）
    "fra3.landx.dev", // Frankfurt3（仅 UDP）
    "ny1.landx.dev",  // New York (TeraSwitch)
    "tyo1.landx.dev", // Tokyo (TeraSwitch)
    "lon1.landx.dev", // London (TeraSwitch)
    "sgp1.landx.dev", // Singapore (TeraSwitch)
    "slc1.landx.dev", // Salt Lake City（仅 UDP）
    "dub1.landx.dev", // Dublin（仅 UDP）
];

/// LandX UDP 发送客户端（无连接状态，不需要重连/保活）。
///
/// # 多入口扇出（`targets`）
///
/// 可以把**同一个包**同时发给多台同城机器（阿姆斯特丹 `ams1`/`ams2`，
/// 法兰克福 `fra1`/`fra2`/`fra3`），覆盖不同 provider / 不同线路。
/// 包内容完全相同 ⇒ 服务端看到的是同一笔交易。
///
/// 由环境变量 `LANDX_FANOUT` 控制（默认只发主入口），见
/// [`crate::platform_clients::landx::resolve_fanout`]。
#[derive(Clone)]
pub struct LandXUdp {
    socket: Arc<UdpSocket>,
    /// (host, 已解析地址)，主入口在前。
    targets: Vec<(String, SocketAddr)>,
    /// 已校验为 12 字节的 API key。
    api_key: [u8; LANDX_API_KEY_LEN],
    /// MTU 告警只打一次，别在热路径刷屏。
    mtu_warned: Arc<AtomicBool>,
}

impl LandXUdp {
    /// 单笔交易最低 tip：与 HTTP 通道一致（0.001 SOL）。
    pub const MIN_TIP_AMOUNT_TX: u64 = LandX::MIN_TIP_AMOUNT_TX;

    /// 区域 → UDP **主**机房（**包含仅 UDP 的 4 个**，这是相比 HTTP 通道多出来的）。
    pub fn endpoint_for(region: Region) -> &'static str {
        match region {
            Region::Amsterdam => LANDX_UDP_HOSTS[0],                       // ams1
            Region::Frankfurt => LANDX_UDP_HOSTS[2],                       // fra1
            Region::Frankfurt2 => LANDX_UDP_HOSTS[3],                      // fra2（仅 UDP）
            Region::NewYork | Region::Pittsburgh | Region::LosAngeles => LANDX_UDP_HOSTS[5], // ny1
            Region::Tokyo => LANDX_UDP_HOSTS[6],                           // tyo1
            Region::London => LANDX_UDP_HOSTS[7],                          // lon1
            Region::Dublin => LANDX_UDP_HOSTS[10],                         // dub1（仅 UDP）
            Region::Singapore => LANDX_UDP_HOSTS[8],                       // sgp1
            Region::SaltLakeCity => LANDX_UDP_HOSTS[9],                    // slc1（仅 UDP）
            Region::Limburg | Region::Lithuania | Region::Unknown => LANDX_UDP_HOSTS[2], // fra1
        }
    }

    pub fn get_host() -> String {
        Self::endpoint_for(*REGION).to_string()
    }

    /// 按 region + `LANDX_FANOUT` 算出最终的 host 列表（主入口在前、已去重）。
    ///
    /// UDP 比 HTTP 多 4 个「仅 UDP」机房，所以 `region` 扇出在法兰克福会得到
    /// `fra1 + fra2 + fra3` 三台，而 HTTP 只有 `fra1`。
    pub fn endpoints_for(region: Region) -> Vec<String> {
        let primary = Self::endpoint_for(region);
        // 默认开启：同城多机器（法兰克福 fra1+fra2+fra3）。
        let spec = std::env::var("LANDX_FANOUT").unwrap_or_else(|_| "region".to_string());
        match crate::platform_clients::landx::resolve_fanout(primary, LANDX_UDP_HOSTS, &spec) {
            Ok(list) => list,
            Err(e) => {
                warn!("[LandXUdp] LANDX_FANOUT 解析失败，回退单入口: {e}");
                vec![primary.to_string()]
            }
        }
    }

    /// 从环境变量 `LANDX_API_KEY` + `REGION` 构造。
    pub async fn new() -> Result<Self, String> {
        let key = std::env::var("LANDX_API_KEY").map_err(|e| format!("LANDX_API_KEY env var required: {e}"))?;
        Self::init_with(key, *REGION).await
    }

    /// 显式构造：校验 key、解析每个 host、绑一个 UDP socket。
    pub async fn init_with(api_key: impl Into<String>, region: Region) -> Result<Self, String> {
        let api_key = parse_api_key(&api_key.into())?;

        let mut targets = Vec::new();
        for host in Self::endpoints_for(region) {
            match resolve(&host).await {
                Ok(addr) => targets.push((host, addr)),
                // 主入口失败是致命的；追加的入口失败只告警跳过
                Err(e) if targets.is_empty() => return Err(e),
                Err(e) => warn!("[LandXUdp] 跳过 {host}: {e}"),
            }
        }

        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| format!("bind udp socket: {e}"))?;

        info!(
            "[LandXUdp] 目标 {} 台: {}",
            targets.len(),
            targets
                .iter()
                .map(|(h, _)| h.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        Ok(Self {
            socket: Arc::new(socket),
            targets,
            api_key,
            mtu_warned: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 提交一笔**已签名**的交易（wire 字节）：同一个包打到所有目标。
    ///
    /// 返回 `Ok` 仅代表至少有一台的内核收下了包 —— UDP 没有回执。
    pub async fn send_transaction_bytes(&self, wire_tx: &[u8]) -> Result<(), String> {
        let packet = encode_packet(&self.api_key, wire_tx)?;

        if packet.len() > MTU_WARN_BYTES && !self.mtu_warned.swap(true, Ordering::Relaxed) {
            warn!(
                "[LandXUdp] 包体 {} 字节 > {MTU_WARN_BYTES}（1500 MTU），会被 IP 分片；\
                 丢任一分片整包作废。大交易建议走 HTTP 通道。",
                packet.len()
            );
        }

        // 允许发多份（UDP 会丢包；同一签名重复到达无害，链上只认一次）。
        // 默认 1 份，设 LANDX_UDP_COPIES=2 可增强抗丢包。
        let copies = udp_copies();

        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();

        for (host, addr) in &self.targets {
            for _ in 0..copies {
                match self.socket.send_to(&packet, *addr).await {
                    Ok(_) => {
                        ok += 1;
                        continue;
                    }
                    // 失败时重解析一次（机房切 IP / DNS 过期）
                    Err(e) => {
                        warn!("[LandXUdp] sendto {host}({addr}) 失败: {e}，重解析后重试");
                        match resolve(host).await {
                            Ok(fresh) => match self.socket.send_to(&packet, fresh).await {
                                Ok(_) => ok += 1,
                                Err(e2) => errors.push(format!("{host}({addr}): {e2}")),
                            },
                            Err(e2) => errors.push(format!("{host}: {e2}")),
                        }
                    }
                }
            }
        }

        if ok == 0 {
            return Err(format!(
                "all {} landx udp target(s) failed: {}",
                self.targets.len(),
                errors.join(" | ")
            ));
        }
        Ok(())
    }

    /// UDP 无连接，这里只用于观测：socket 是否还在。
    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.socket.local_addr().map_err(|e| e.to_string())
    }

    /// 全部目标地址（主入口在前）。
    pub fn targets(&self) -> Vec<SocketAddr> {
        self.targets.iter().map(|(_, a)| *a).collect()
    }

    /// 主入口地址。
    pub fn target(&self) -> Option<SocketAddr> {
        self.targets.first().map(|(_, a)| *a)
    }

    /// 主入口 host。
    pub fn host(&self) -> &str {
        self.targets.first().map(|(h, _)| h.as_str()).unwrap_or("")
    }
}

/// 编码 UDP 包体：`[12 字节 key][wire tx]`。没有帧头、没有长度前缀。
pub fn encode_packet(api_key: &[u8; LANDX_API_KEY_LEN], wire_tx: &[u8]) -> Result<Vec<u8>, String> {
    if wire_tx.is_empty() {
        return Err("wire 交易为空".to_string());
    }
    let total = LANDX_API_KEY_LEN + wire_tx.len();
    if total > MAX_UDP_PAYLOAD {
        return Err(format!(
            "包体 {total} 字节超过 UDP 上限 {MAX_UDP_PAYLOAD}（交易 {} 字节）",
            wire_tx.len()
        ));
    }
    let mut packet = Vec::with_capacity(total);
    packet.extend_from_slice(api_key);
    packet.extend_from_slice(wire_tx);
    Ok(packet)
}

/// API key 必须是**恰好 12 字节**：它直接拼在包体最前面，没有长度前缀，
/// 长度错了服务端只会静默丢弃，所以在这里提前拒掉。
pub fn parse_api_key(api_key: &str) -> Result<[u8; LANDX_API_KEY_LEN], String> {
    let bytes = api_key.trim().as_bytes();
    if bytes.len() != LANDX_API_KEY_LEN {
        return Err(format!(
            "LandX API key 必须是 {LANDX_API_KEY_LEN} 字节，实际 {} 字节",
            bytes.len()
        ));
    }
    let mut key = [0u8; LANDX_API_KEY_LEN];
    key.copy_from_slice(bytes);
    Ok(key)
}

/// 每包发几份（默认 1）。
fn udp_copies() -> usize {
    std::env::var("LANDX_UDP_COPIES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| (1..=4).contains(n))
        .unwrap_or(1)
}

async fn resolve(host: &str) -> Result<SocketAddr, String> {
    tokio::net::lookup_host((host, LANDX_UDP_PORT))
        .await
        .map_err(|e| format!("resolve {host}:{LANDX_UDP_PORT}: {e}"))?
        .next()
        .ok_or_else(|| format!("resolve {host}:{LANDX_UDP_PORT}: no address"))
}

impl fmt::Display for LandXUdp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LandXUdp({}x{}:{})", self.targets.len(), self.host(), LANDX_UDP_PORT)
    }
}

#[async_trait::async_trait]
impl SendTx for LandXUdp {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("landx udp send: ", {
            let wire = serialize_transaction_wire(tx)?;
            self.send_transaction_bytes(&wire).await?;
            // ⚠️ 到这只代表包交给内核了：UDP 无回执。
            info!(
                "[LandXUdp] {}:{} 已投递（UDP 无回执）sig={}",
                self.host(),
                LANDX_UDP_PORT,
                tx.signatures.first().map(|s| s.to_string()).unwrap_or_default()
            );
            Ok(())
        })
    }
}

impl crate::platform_clients::BuildTx for LandXUdp {
    fn platform(&self) -> PlatformName {
        // 与 HTTP 通道同一平台语义（tip 账户 / 最低 tip 一致）。
        PlatformName::LandX
    }

    fn get_tip_address(&self) -> Pubkey {
        *LANDX_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| LANDX_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        LANDX_TIP_ACCOUNTS.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8; LANDX_API_KEY_LEN] = b"ABCDEFGHIJKL";

    /// 包体布局：**前 12 字节就是 key**，后面紧跟原始交易字节，没有任何额外前缀。
    #[test]
    fn packet_layout_matches_spec() {
        let tx = vec![0xab, 0xcd, 0xef];
        let pkt = encode_packet(KEY, &tx).unwrap();
        assert_eq!(pkt.len(), 12 + tx.len());
        assert_eq!(&pkt[0..12], KEY, "前 12 字节必须是 API key");
        assert_eq!(&pkt[12..], tx.as_slice(), "12 之后必须是原始 wire 字节");
        // 没有长度前缀 / magic / 分隔符
        assert_eq!(pkt[12], 0xab);
    }

    /// 空交易直接拒（服务端也会丢，别浪费一个包）。
    #[test]
    fn empty_tx_rejected() {
        assert!(encode_packet(KEY, &[]).is_err());
    }

    /// 单包不能超过 UDP 载荷上限。
    #[test]
    fn payload_limit_enforced() {
        let max_tx = vec![0u8; MAX_UDP_PAYLOAD - LANDX_API_KEY_LEN];
        assert!(encode_packet(KEY, &max_tx).is_ok());

        let too_big = vec![0u8; MAX_UDP_PAYLOAD - LANDX_API_KEY_LEN + 1];
        assert!(encode_packet(KEY, &too_big).is_err());
    }

    /// key 必须是恰好 12 字节：短一位 / 长一位都拒。
    #[test]
    fn api_key_must_be_exactly_12_bytes() {
        assert!(parse_api_key("ABCDEFGHIJKL").is_ok());
        assert!(parse_api_key("  ABCDEFGHIJKL  ").is_ok(), "两侧空白应被 trim");
        assert!(parse_api_key("ABCDEFGHIJK").is_err(), "11 字节");
        assert!(parse_api_key("ABCDEFGHIJKLM").is_err(), "13 字节");
        assert!(parse_api_key("").is_err());
        assert_eq!(parse_api_key("ABCDEFGHIJKL").unwrap(), *KEY);
    }

    /// 区域映射必须把「仅 UDP 机房」真正用起来。
    #[test]
    fn udp_only_regions_are_reachable() {
        assert_eq!(LandXUdp::endpoint_for(Region::Frankfurt2), "fra2.landx.dev");
        assert_eq!(LandXUdp::endpoint_for(Region::SaltLakeCity), "slc1.landx.dev");
        assert_eq!(LandXUdp::endpoint_for(Region::Dublin), "dub1.landx.dev");
    }

    /// 全部 11 个机房都在表里且不重复；主机名都是裸域名。
    #[test]
    fn host_table_is_complete_and_unique() {
        assert_eq!(LANDX_UDP_HOSTS.len(), 11);
        let mut uniq = LANDX_UDP_HOSTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 11, "LANDX_UDP_HOSTS 有重复");
        for h in LANDX_UDP_HOSTS {
            assert!(!h.contains(':'), "UDP 主机名不应带端口: {h}");
            assert!(!h.contains("://"), "UDP 主机名不应带 scheme: {h}");
            assert!(h.ends_with(".landx.dev"), "{h}");
        }
        // HTTP 能走的 7 个必须都在
        for h in crate::platform_clients::landx::LANDX_HTTP_HOSTS {
            let bare = h.trim_start_matches("http://");
            assert!(LANDX_UDP_HOSTS.contains(&bare), "UDP 表缺 {bare}");
        }
        // 仅 UDP 的 4 个也必须都在
        for h in crate::platform_clients::landx::LANDX_UDP_ONLY_HOSTS {
            assert!(LANDX_UDP_HOSTS.contains(h), "UDP 表缺 {h}");
        }
    }

    /// 法兰克福的 `region` 扇出在 UDP 侧能拿到 3 台（fra1/fra2/fra3，后两台仅 UDP）。
    #[test]
    fn frankfurt_udp_region_fanout_covers_three_hosts() {
        let list = crate::platform_clients::landx::resolve_fanout(
            "fra1.landx.dev",
            LANDX_UDP_HOSTS,
            "region",
        )
        .unwrap();
        assert_eq!(
            list,
            vec![
                "fra1.landx.dev".to_string(),
                "fra2.landx.dev".to_string(),
                "fra3.landx.dev".to_string(),
            ]
        );
    }

    /// UDP 侧能看到全部 11 台，`all` 会全部包含。
    #[test]
    fn udp_region_fanout_for_amsterdam() {
        let list = crate::platform_clients::landx::resolve_fanout(
            "ams1.landx.dev",
            LANDX_UDP_HOSTS,
            "region",
        )
        .unwrap();
        assert_eq!(
            list,
            vec!["ams1.landx.dev".to_string(), "ams2.landx.dev".to_string()]
        );

        let all = crate::platform_clients::landx::resolve_fanout(
            "ams1.landx.dev",
            LANDX_UDP_HOSTS,
            "all",
        )
        .unwrap();
        assert_eq!(all.len(), LANDX_UDP_HOSTS.len());
    }

    /// 常规区域映射。
    #[test]
    fn endpoint_per_region() {
        let cases = [
            (Region::Amsterdam, "ams1.landx.dev"),
            (Region::Frankfurt, "fra1.landx.dev"),
            (Region::NewYork, "ny1.landx.dev"),
            (Region::Tokyo, "tyo1.landx.dev"),
            (Region::London, "lon1.landx.dev"),
            (Region::Singapore, "sgp1.landx.dev"),
            (Region::Limburg, "fra1.landx.dev"),
            (Region::Unknown, "fra1.landx.dev"),
        ];
        for (region, expected) in cases {
            assert_eq!(LandXUdp::endpoint_for(region), expected, "{region:?}");
        }
    }

    /// 协议常量：端口 10000、key 12 字节、MTU 阈值 1472。
    #[test]
    fn protocol_constants() {
        assert_eq!(LANDX_UDP_PORT, 10000);
        assert_eq!(LANDX_API_KEY_LEN, 12);
        assert_eq!(MTU_WARN_BYTES, 1472);
        // 典型 v1 交易（4096）必然超过 MTU，会被分片
        assert!(LANDX_API_KEY_LEN + 4096 > MTU_WARN_BYTES);
    }
}
