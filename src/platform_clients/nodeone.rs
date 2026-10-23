use std::fmt;
impl fmt::Display for NodeOne {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeOne")
    }
}
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

/// Node1 tip 地址（官方 24 个，随机选取以分散负载）。
/// 是否使用以 `tip_recvs()` 为准，`get_tip_address()` 随机挑一个。
pub const NODEONE_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("Node1DWLLHtpKXQUe1FvLgANTgx9qknWDBMCgB9Dbc8"),
    pubkey!("Node1DXv6DFbzJKCfJF3wcsLg1KYm3AHAMAq7jFTZ44"),
    pubkey!("Node1GcMxYVrr5FABQxbvWn7AY9UjeurZUu2BCcUEJb"),
    pubkey!("Node1H5SG2b7Qa9KRrDGtPfffQ8JsdYePur4RcJViqq"),
    pubkey!("Node1Jai3s5yU5kXZnrECbupzhdVmPh3p1MYRnwvruH"),
    pubkey!("Node1JvDo7mKGh3hqXVXTeohksejQuh385tnoEsYsxS"),
    pubkey!("Node1JwcWbikWff65JdVD16vTor2TYSaFKvHWQhn2ZK"),
    pubkey!("Node1KGHLPrvZbAFqaauTFFnMHYU6MshJWTRBrUQfYg"),
    pubkey!("Node1Vm76GupuHSvHZJr4Unojz1EBGAQ1jjaWzLhLzX"),
    pubkey!("Node1bqVym2A6wMcXaaoEns91GsY78A3vZFNWahxkFY"),
    pubkey!("Node1cEZ1Dgmi188mvm6fsTW5vw4nGRArcuXcssBq7i"),
    pubkey!("Node1cecy1NeGSLburSQhZp75TZN6o58aTaRJWXTgnX"),
    pubkey!("Node1dkDBeE3ZYyCE1YXZvxs6nK91fFmt1NxasZsCf7"),
    pubkey!("Node1nF4fmy7Za7LrTLQEatfy77KW5xZVmFsg4PGwWf"),
    pubkey!("Node1o1bivr8Xeb2qyLn4LDWdNVnFYNS2CQwx2zxHDq"),
    pubkey!("Node1oLXzJ9A1Q5RxvEXfbTj6JDU5BaZhBhJRXEgKpg"),
    pubkey!("Node1oe6BgwdxJXdkoXq24XZo7oMS7UQExonUhAcfaq"),
    pubkey!("Node1ovdUHugTxbCUydcEbXRG71BFWM1J3Zp7WPiqGj"),
    pubkey!("Node1pyyf5SKsNVAVAB7pS1J6KZjesbRuT3MHAn1M5Q"),
    pubkey!("Node1qdrBBhNu73jnvxZNJPYXtusdeCapbP8A3C8PLK"),
    pubkey!("Node1smSyGSEoPWrANwbhfeRBSkHfq5PF8vsE3r4Be4"),
    pubkey!("Node1vmD7f7GYF5ghRS27Rp4fNBdtCGEcSzkiXjdpnP"),
    pubkey!("Node1yVhvYnU8fqgiHULQzkbJnfgmUCxesPYJBz4STe"),
    pubkey!("Node1zoA9spwBsUQS8dZNmCjEoyyJKFybgbTKCs6p3K"),
];

// Node1 接入点（官方 8 个地区，HTTP/HTTPS 均可，建议长连接 Keep-Alive）
pub const NODEONE_ENDPOINT: &[&str] = &[
    "https://ny.node1.me",   // New York
    "https://ams.node1.me",  // Amsterdam
    "https://fra.node1.me",  // Frankfurt
    "https://lon.node1.me",  // London
    "https://tk.node1.me",   // Tokyo
    "https://sgp.node1.me",  // Singapore
    "https://fra2.node1.me", // Frankfurt 2
    "https://dub.node1.me",  // Dublin
];

/// 官方 8 个机房的**裸 host**（fanout 解析用）。
///
/// 官网对每个机房同时提供 `https://` 与 `http://`；`http://` 少一次 TLS 往返，
/// 所以默认走 http（可用 `NODEONE_SCHEME=https` 切回）。
pub const NODEONE_HOSTS: &[&str] = &[
    "ny.node1.me",
    "ams.node1.me",
    "fra.node1.me",
    "lon.node1.me",
    "tk.node1.me",
    "sgp.node1.me",
    "fra2.node1.me",
    "dub.node1.me",
];

/// **同城多机器**：法兰克福有 `fra` + `fra2` 两台（不同机器），其余城市单台。
pub const NODEONE_CITY_GROUPS: &[(&str, &[&str])] = &[(
    "fra.node1.me",
    &["fra.node1.me", "fra2.node1.me"],
)];

/// 取某个 host 所在城市的全部机器（没有分组返回空切片）。
pub fn city_group(host: &str) -> &'static [&'static str] {
    NODEONE_CITY_GROUPS
        .iter()
        .find(|(key, _)| *key == host)
        .map(|(_, hosts)| *hosts)
        .unwrap_or(&[])
}

/// Node1 HTTP 交易发送客户端。
///
/// # 多入口扇出（`endpoints`）
///
/// 可以持有多个入口（同城多机器），`send_tx` 把**同一笔已签名交易**并发打到所有入口。
/// 由环境变量 `NODEONE_FANOUT` 控制（默认 `region`），见
/// [`crate::platform_clients::resolve_host_fanout`]。
#[derive(Clone)]
pub struct NodeOne {
    /// 提交 URL 列表（含 scheme），主入口在前。
    pub endpoints: Vec<String>,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl NodeOne {
    pub const MIN_TIP_AMOUNT_TX: u64 = 100_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 2_000_000; // 批量交易最低 tip

    /// 按区域返回 Node1 接入点（纯函数，不依赖全局状态）。
    ///
    /// Node1 没有机房的区域就近回退：
    /// - `Pittsburgh` / `LosAngeles` / `SaltLakeCity` → New York
    /// - `Limburg` / `Lithuania` → Amsterdam
    /// - `Unknown` → Frankfurt
    pub fn endpoint_for(region: Region) -> &'static str {
        match region {
            Region::NewYork | Region::Pittsburgh | Region::LosAngeles | Region::SaltLakeCity => {
                NODEONE_ENDPOINT[0]
            }
            Region::Amsterdam | Region::Limburg | Region::Lithuania => NODEONE_ENDPOINT[1],
            Region::Frankfurt => NODEONE_ENDPOINT[2],
            Region::London => NODEONE_ENDPOINT[3],
            Region::Tokyo => NODEONE_ENDPOINT[4],
            Region::Singapore => NODEONE_ENDPOINT[5],
            Region::Frankfurt2 => NODEONE_ENDPOINT[6],
            Region::Dublin => NODEONE_ENDPOINT[7],
            Region::Unknown => NODEONE_ENDPOINT[2],
        }
    }

    /// 本机地区的主入口 URL —— **必须和真正提交用的 scheme 一致**。
    ///
    /// 历史坑：这里曾直接返回 `endpoint_for()` 的 `https://` 常量，而提交已经改成
    /// 默认 `http://`（见 [`Self::scheme`]），于是保活一直在预热 443/TLS，
    /// 真正发交易却走 80 —— 连接根本没被保住。
    pub fn get_endpoint() -> String {
        format!("{}://{}", Self::scheme(), Self::host_for(*REGION))
    }

    /// 裸 host（去掉 scheme）→ 由 [`Self::endpoint_for`] 反推。
    pub fn host_for(region: Region) -> &'static str {
        Self::endpoint_for(region)
            .trim_start_matches("https://")
            .trim_start_matches("http://")
    }

    /// 提交用的 scheme：默认 `http`（少一次 TLS），`NODEONE_SCHEME=https` 可切回。
    pub fn scheme() -> &'static str {
        match std::env::var("NODEONE_SCHEME").as_deref() {
            Ok(v) if v.eq_ignore_ascii_case("https") => "https",
            _ => "http",
        }
    }

    /// 按 region + `NODEONE_FANOUT` 算出最终提交 URL 列表。
    ///
    /// 默认 `region`：法兰克福会同时打 `fra` 和 `fra2` 两台机器。
    pub fn endpoints_for(region: Region) -> Vec<String> {
        let primary = Self::host_for(region);
        let spec = std::env::var("NODEONE_FANOUT").unwrap_or_else(|_| "region".to_string());
        let scheme = Self::scheme();

        match crate::platform_clients::resolve_host_fanout(primary, NODEONE_HOSTS, city_group, &spec) {
            Ok(hosts) => hosts.into_iter().map(|h| format!("{scheme}://{h}")).collect(),
            Err(e) => {
                log::warn!("[node1] NODEONE_FANOUT 解析失败，回退单入口: {e}");
                vec![format!("{scheme}://{primary}")]
            }
        }
    }

    /// 主入口。
    pub fn primary(&self) -> &str {
        self.endpoints.first().map(|s| s.as_str()).unwrap_or("")
    }

    pub fn new() -> Self {
        NodeOne {
            endpoints: Self::endpoints_for(*REGION),
            auth_token: std::env::var("NODEONE_KEY").unwrap_or_default(),
            http_client: HTTP_CLIENT.clone(),
        }
    }

    /// 显式构造：调用方负责提供 key 和 region，不读取任何环境变量。
    pub fn init_with(key: impl Into<String>, region: Region) -> Self {
        NodeOne {
            endpoints: Self::endpoints_for(region),
            auth_token: key.into(),
            http_client: HTTP_CLIENT.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for NodeOne {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        log_time!("node1 send: ", {
            let tx_base64 = tx.to_base64().map_err(|e| e.to_string())?;
            // 同一份 tx_base64 并发打到所有入口（同签名）。
            let body = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendTransaction",
                "params": [
                    tx_base64,
                    { "encoding": "base64", "skipPreflight": true }
                ],
            });

            let mut set = tokio::task::JoinSet::new();
            for endpoint in &self.endpoints {
                let client = self.http_client.clone();
                let url = endpoint.clone();
                let key = self.auth_token.clone();
                let body = body.clone();
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
                            (url, status.is_success(), format!("HTTP {status} {text}"))
                        }
                        Err(e) => (url, false, format!("send error: {e}")),
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
                            info!("node1: {label} ok: {detail}");
                        } else {
                            log::warn!("[node1] {label} failed: {detail}");
                            errors.push(format!("{label}: {detail}"));
                        }
                    }
                    Err(e) => errors.push(format!("task join error: {e}")),
                }
            }

            if ok == 0 {
                return Err(format!(
                    "all {} node1 endpoint(s) failed: {}",
                    self.endpoints.len(),
                    errors.join(" | ")
                ));
            }
            Ok(())
        })
    }
}

impl crate::platform_clients::BuildTx for NodeOne {
    fn get_tip_address(&self) -> Pubkey {
        *NODEONE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| NODEONE_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Nodeone
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    fn tip_recvs(&self) -> Vec<Pubkey> {
        NODEONE_TIP_ACCOUNTS.to_vec()
    }
    // 使用默认实现，无需重写 build_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 8 个机房都在 host 表里，且覆盖 `endpoint_for` 的所有返回值。
    #[test]
    fn host_table_covers_all_regions() {
        assert_eq!(NODEONE_HOSTS.len(), 8);
        let mut uniq = NODEONE_HOSTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 8, "NODEONE_HOSTS 有重复");

        for r in Region::ALL {
            let host = NodeOne::host_for(*r);
            assert!(NODEONE_HOSTS.contains(&host), "{r:?} → {host} 不在 host 表里");
            assert!(!host.contains("://"), "host_for 应返回裸 host: {host}");
        }
    }

    /// 法兰克福同城的两台机器（fra + fra2）必须能被 `region` 扇出同时选中。
    #[test]
    fn frankfurt_city_group_has_two_machines() {
        let list = crate::platform_clients::resolve_host_fanout(
            "fra.node1.me",
            NODEONE_HOSTS,
            city_group,
            "region",
        )
        .unwrap();
        assert_eq!(
            list,
            vec!["fra.node1.me".to_string(), "fra2.node1.me".to_string()]
        );

        // 单机房城市扇出后仍然只有自己
        let ams = crate::platform_clients::resolve_host_fanout(
            "ams.node1.me",
            NODEONE_HOSTS,
            city_group,
            "region",
        )
        .unwrap();
        assert_eq!(ams, vec!["ams.node1.me".to_string()]);
    }

    /// 默认配置下至少有一个入口，且 scheme 是 http（少一次 TLS）。
    #[test]
    fn endpoints_default_scheme_and_nonempty() {
        let eps = NodeOne::endpoints_for(Region::Frankfurt);
        assert!(!eps.is_empty());
        for ep in &eps {
            assert!(
                ep.starts_with("http://") || ep.starts_with("https://"),
                "{ep}"
            );
            assert!(ep.contains("node1.me"), "{ep}");
        }
        assert_eq!(eps[0], format!("{}://fra.node1.me", NodeOne::scheme()));
    }

    /// 官方 24 个 tip 地址，全部唯一且非空（防止复制粘贴漏改/重复）。
    #[test]
    fn test_tip_accounts_unique_and_complete() {
        assert_eq!(NODEONE_TIP_ACCOUNTS.len(), 24);
        let mut uniq = NODEONE_TIP_ACCOUNTS.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 24, "NODEONE_TIP_ACCOUNTS 存在重复地址");
    }

    /// 8 个接入点按区域一一对应。
    #[test]
    fn test_endpoint_per_region() {
        assert_eq!(
            NodeOne::endpoint_for(Region::NewYork),
            "https://ny.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Amsterdam),
            "https://ams.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Frankfurt),
            "https://fra.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::London),
            "https://lon.node1.me"
        );
        assert_eq!(NodeOne::endpoint_for(Region::Tokyo), "https://tk.node1.me");
        assert_eq!(
            NodeOne::endpoint_for(Region::Singapore),
            "https://sgp.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Frankfurt2),
            "https://fra2.node1.me"
        );
        assert_eq!(
            NodeOne::endpoint_for(Region::Dublin),
            "https://dub.node1.me"
        );
        // 无 Node1 机房的区域回退到 New York
        assert_eq!(
            NodeOne::endpoint_for(Region::LosAngeles),
            "https://ny.node1.me"
        );
    }
}
