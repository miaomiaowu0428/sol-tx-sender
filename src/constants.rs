use crate::platform_clients::Region;
use reqwest::Client;
use solana_sdk::pubkey;
use std::sync::{Arc, LazyLock};

/// 全局共享的 HTTP 客户端。
///
/// # 为什么这些参数很关键
///
/// 所有平台共用**同一个** `Client`，连接池按 origin（scheme+host+port）复用，
/// 所以这里配置的东西直接决定 `send_tx` 是「复用热连接」还是「每次重新
/// DNS + TCP + TLS 握手」——低延迟发交易场景下后者是致命的。
///
/// - `tcp_nodelay(true)`：禁用 Nagle，避免小包被攒着延迟发送。
/// - `tcp_keepalive(30s)`：OS 层对着空闲连接发 TCP keepalive，防止中间
///   NAT/LB 静默掐断连接（否则下一次 `send_tx` 会撞上坏连接再重试）。
/// - `pool_idle_timeout(90s)`：连接池淘汰空闲连接的时限；keep-alive 探测
///   间隔必须**小于**它，否则连接刚被淘汰又要重建。
/// - `pool_max_idle_per_host`：每个 origin 至少留几条热连接。
pub static HTTP_CLIENT: LazyLock<Arc<Client>> = LazyLock::new(|| {
    Arc::new(
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .tcp_nodelay(true)
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(16)
            .build()
            .expect("Failed to create HTTP client"),
    )
});

pub static REGION: LazyLock<Region> = LazyLock::new(|| {
    let region_str = std::env::var("REGION").unwrap_or_else(|_| "Frankfurt".to_string());
    Region::from(region_str)
});

pub static MEMO_PROGRAM: LazyLock<solana_sdk::pubkey::Pubkey> =
    LazyLock::new(|| pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"));
