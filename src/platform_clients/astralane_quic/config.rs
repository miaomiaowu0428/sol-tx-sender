//! Astralane QUIC endpoint configuration.
//!
//! 官方文档（2026-10 同步）：
//! <https://astralane.gitbook.io/docs/low-latency/submit-transactions/quic-transaction-submission>
//!
//! 与 HTTP/Iris 侧一致，官方目前提供 11 个 QUIC 区域端点：
//! `fr` / `fr2` / `la` / `jp` / `ny` / `ams` / `ams2` / `lim` / `sg` / `lit` / `lon`。
//! QUIC 用 `IP:7000` 直连（IP 比域名更快）；文档同时列出 **MEV-protected** 端口
//! `:9000`，本客户端默认只走 `:7000`，需要 MEV 保护时把对应端点端口换成 `:9000`。
//!
//! 端点明细与 HTTP 侧的对应关系见 `docs/astralane.md`。

use crate::platform_clients::Region;

/// QUIC endpoints by region, matching the official Astralane documentation.
///
/// 顺序与 [`get_quic_endpoint`] 的映射保持一致，便于测试与遍历。
pub const ASTRALANE_QUIC_ENDPOINTS: &[&str] = &[
    "185.191.117.97:7000", // Frankfurt（recommended）
    "45.139.132.160:7000", // Frankfurt 2
    "74.118.142.151:7000", // Los Angeles（旧文档称 San Francisco）
    "189.1.164.31:7000",   // Tokyo
    "64.130.45.19:7000",   // New York
    "64.130.43.43:7000",   // Amsterdam（recommended）
    "84.32.186.73:7000",   // Amsterdam 2（Cherry Servers）
    "162.19.222.232:7000", // Limburg
    "67.209.54.176:7000",  // Singapore
    "84.32.97.47:7000",    // Lithuania
    "63.254.168.71:7000",  // London
];

/// Get the appropriate QUIC endpoint based on region.
///
/// 没有 Astralane 机房的区域就近回退：
/// - `Dublin` → London
/// - `SaltLakeCity` → Los Angeles
/// - `Pittsburgh` → New York
/// - `Unknown` → Frankfurt
pub fn get_quic_endpoint(region: &Region) -> &'static str {
    match region {
        Region::Frankfurt => ASTRALANE_QUIC_ENDPOINTS[0],  // Frankfurt (recommended)
        Region::Frankfurt2 => ASTRALANE_QUIC_ENDPOINTS[1], // Frankfurt 2
        Region::LosAngeles => ASTRALANE_QUIC_ENDPOINTS[2], // Los Angeles
        Region::Tokyo => ASTRALANE_QUIC_ENDPOINTS[3],      // Tokyo
        Region::NewYork => ASTRALANE_QUIC_ENDPOINTS[4],    // New York
        Region::Amsterdam => ASTRALANE_QUIC_ENDPOINTS[5],  // Amsterdam (recommended)
        Region::Limburg => ASTRALANE_QUIC_ENDPOINTS[7],    // Limburg
        Region::Singapore => ASTRALANE_QUIC_ENDPOINTS[8],  // Singapore
        Region::Lithuania => ASTRALANE_QUIC_ENDPOINTS[9],  // Lithuania
        Region::London | Region::Dublin => ASTRALANE_QUIC_ENDPOINTS[10], // London
        Region::SaltLakeCity => ASTRALANE_QUIC_ENDPOINTS[2], // 就近回退 Los Angeles
        Region::Pittsburgh => ASTRALANE_QUIC_ENDPOINTS[4], // 就近回退 New York
        Region::Unknown => ASTRALANE_QUIC_ENDPOINTS[0],    // Default to Frankfurt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quic_endpoints_complete_and_unique() {
        assert_eq!(ASTRALANE_QUIC_ENDPOINTS.len(), 11);
        assert!(ASTRALANE_QUIC_ENDPOINTS.iter().all(|e| e.ends_with(":7000")));

        let mut uniq = ASTRALANE_QUIC_ENDPOINTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 11, "ASTRALANE_QUIC_ENDPOINTS 存在重复地址");
    }

    #[test]
    fn quic_endpoint_per_region() {
        let cases = [
            (Region::Frankfurt, "185.191.117.97:7000"),
            (Region::Frankfurt2, "45.139.132.160:7000"),
            (Region::LosAngeles, "74.118.142.151:7000"),
            (Region::Tokyo, "189.1.164.31:7000"),
            (Region::NewYork, "64.130.45.19:7000"),
            (Region::Amsterdam, "64.130.43.43:7000"),
            (Region::Limburg, "162.19.222.232:7000"),
            (Region::Singapore, "67.209.54.176:7000"),
            (Region::Lithuania, "84.32.97.47:7000"),
            (Region::London, "63.254.168.71:7000"),
            // 就近回退
            (Region::Dublin, "63.254.168.71:7000"),
            (Region::SaltLakeCity, "74.118.142.151:7000"),
            (Region::Pittsburgh, "64.130.45.19:7000"),
            (Region::Unknown, "185.191.117.97:7000"),
        ];
        for (region, expected) in cases {
            assert_eq!(get_quic_endpoint(&region), expected, "region {region:?}");
        }
    }
}
