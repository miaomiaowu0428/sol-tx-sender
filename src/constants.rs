use crate::platform_clients::Region;
use reqwest::Client;
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::env;
use std::sync::{Arc, LazyLock};

pub mod api_config {
    pub const BLOCKRAZOR_KEY: &str = "";
}

pub mod endpoint_config {
    use std::sync::LazyLock;
    use crate::platform_clients::Region;

    // Astralane 端点
    pub const ASTRALANE_ENDPOINTS: &[&str] = &[
        "http://fr.gateway.astralane.io/iris",  // Frankfurt
        "http://lax.gateway.astralane.io/iris", // San Francisco
        "http://jp.gateway.astralane.io/iris",  // Tokyo
        "http://ny.gateway.astralane.io/iris",  // NewYork
        "http://ams.gateway.astralane.io/iris", // Amsterdam
    ];

    // Blockrazor 端点
    pub const BLOCKRAZOR_ENDPOINTS: &[&str] = &[
        "http://frankfurt.solana.blockrazor.xyz:443/sendTransaction", // Frankfurt
        "http://newyork.solana.blockrazor.xyz:443/sendTransaction",   // NewYork
        "http://tokyo.solana.blockrazor.xyz:443/sendTransaction",     // Tokyo
        "http://amsterdam.solana.blockrazor.xyz:443/sendTransaction", // Amsterdam
    ];

    // Helius 端点
    pub const HELIUS_ENDPOINTS: &[&str] = &[
        "http://ewr-sender.helius-rpc.com/fast", // NY
        "http://ams-sender.helius-rpc.com/fast", // Amsterdam
        "http://fra-sender.helius-rpc.com/fast", // Frankfurt
        "http://lon-sender.helius-rpc.com/fast", // London
        "http://slc-sender.helius-rpc.com/fast", // Salt Lake City
        "http://tyo-sender.helius-rpc.com/fast", // Tokyo
        "http://sg-sender.helius-rpc.com/fast",  // Singapore
    ];

    // Jito 端点
    pub const JITO_ENDPOINTS: &[&str] = &[
        "https://ny.mainnet.block-engine.jito.wtf",        // NY
        "https://frankfurt.mainnet.block-engine.jito.wtf", // Frankfurt
        "https://amsterdam.mainnet.block-engine.jito.wtf", // Amsterdam
        "https://london.mainnet.block-engine.jito.wtf",    // London
        "https://slc.mainnet.block-engine.jito.wtf",       // Salt Lake City
        "https://tokyo.mainnet.block-engine.jito.wtf",     // Tokyo
        "https://singapore.mainnet.block-engine.jito.wtf", // Singapore
    ];

    // NodeOne 端点
    pub const NODEONE_ENDPOINTS: &[&str] = &[
        "https://ny.node1.me",  // NY
        "https://fra.node1.me", // Frankfurt
        "https://ams.node1.me", // Amsterdam
    ];

    // Temporal 端点
    pub const TEMPORAL_ENDPOINTS: &[&str] = &[
        "http://pit1.nozomi.temporal.xyz/", // Pittsburgh
        "http://tyo1.nozomi.temporal.xyz/", // Tokyo
        "http://sgp1.nozomi.temporal.xyz/", // Singapore
        "http://ewr1.nozomi.temporal.xyz/", // NY
        "http://ams1.nozomi.temporal.xyz/", // Amsterdam
        "http://fra2.nozomi.temporal.xyz/", // Frankfurt
    ];

    // ZeroSlot 端点
    pub const ZEROSLOT_ENDPOINTS: &[&str] = &[
        "https://ny.0slot.trade",   // NewYork
        "http://de1.0slot.trade",   // Frankfurt
        "https://ams.0slot.trade",  // Amsterdam
        "https://jp.0slot.trade",   // Tokyo
        "https://la.0slot.trade",   // LosAngeles
    ];

    /// 根据地区选择最佳端点
    pub fn get_optimal_endpoint(endpoints: &[&str], region: Region) -> String {
        let index = match region {
            Region::NewYork => match endpoints.len() {
                len if len > 0 => 0,  // 通常第一个是 NY
                _ => 0,
            },
            Region::Frankfurt => match endpoints.len() {
                len if len > 1 => 1,  // 通常第二个是 Frankfurt
                _ => 0,
            },
            Region::Amsterdam => match endpoints.len() {
                len if len > 2 => 2,  // 通常第三个是 Amsterdam
                _ => 0,
            },
            Region::London => match endpoints.len() {
                len if len > 3 => 3,  // 通常第四个是 London
                _ => 0,
            },
            Region::SaltLakeCity => match endpoints.len() {
                len if len > 4 => 4,  // 通常第五个是 SLC
                _ => 0,
            },
            Region::Tokyo => match endpoints.len() {
                len if len > 5 => 5,  // 通常第六个是 Tokyo
                len if len > 2 => 2,  // 或者第三个
                _ => 0,
            },
            Region::Singapore => match endpoints.len() {
                len if len > 6 => 6,  // 通常第七个是 Singapore
                _ => 0,
            },
            Region::LosAngeles => match endpoints.len() {
                len if len > 4 => 4,  // 通常第五个是 LA
                _ => 0,
            },
            Region::Pittsburgh => match endpoints.len() {
                len if len > 0 => 0,  // Pittsburgh 通常映射到第一个
                _ => 0,
            },
            _ => 0,  // 默认使用第一个端点
        };
        
        endpoints.get(index).unwrap_or(&endpoints[0]).to_string()
    }

    // 动态端点配置
    pub static ASTRALANE_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("ASTRALANE_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(ASTRALANE_ENDPOINTS, *crate::constants::REGION)
        })
    });

    pub static BLOCKRAZOR_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("BLOCKRAZOR_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(BLOCKRAZOR_ENDPOINTS, *crate::constants::REGION)
        })
    });

    pub static HELIUS_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("HELIUS_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(HELIUS_ENDPOINTS, *crate::constants::REGION)
        })
    });

    pub static JITO_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("JITO_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(JITO_ENDPOINTS, *crate::constants::REGION)
        })
    });

    pub static NODEONE_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("NODEONE_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(NODEONE_ENDPOINTS, *crate::constants::REGION)
        })
    });

    pub static TEMPORAL_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("TEMPORAL_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(TEMPORAL_ENDPOINTS, *crate::constants::REGION)
        })
    });

    pub static ZEROSLOT_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("ZEROSLOT_URL").unwrap_or_else(|_| {
            get_optimal_endpoint(ZEROSLOT_ENDPOINTS, *crate::constants::REGION)
        })
    });
}

pub static HTTP_CLIENT: LazyLock<Arc<Client>> = LazyLock::new(|| {
    Arc::new(
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client"),
    )
});

pub static JSON_RPC_CLIENT: LazyLock<RpcClient> = LazyLock::new(|| {
    RpcClient::new(
        std::env::var("JSON_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
    )
});

pub static REGION: LazyLock<Region> = LazyLock::new(|| {
    let region_str = std::env::var("REGION").unwrap_or_else(|_| "NewYork".to_string());
    Region::from(region_str)
});

pub static PAYER: LazyLock<Arc<Keypair>> = LazyLock::new(|| {
    let payer_path = env::var("PAYER_KEYPAIR_PATH")
        .expect("PAYER_KEYPAIR_PATH environment variable must be set");
    let keypair = solana_sdk::signature::read_keypair_file(&payer_path)
        .unwrap_or_else(|e| panic!("Failed to read keypair from file '{}': {}", payer_path, e));
    log::info!("Using wallet : {}", keypair.pubkey());
    Arc::new(keypair)
});

/// 异步端点保活功能 - 定期ping所有MEV平台端点
pub async fn endpoint_keep_alive() {
    endpoint_keep_alive_with_interval(60).await;
}

/// 带自定义间隔的异步端点保活功能
pub async fn endpoint_keep_alive_with_interval(interval_secs: u64) {
    let client = HTTP_CLIENT.clone();

    // 显示当前使用的端点配置
    log::info!("🚀 Starting endpoint keep-alive with interval: {}s", interval_secs);
    log::info!("📍 Current region: {:?}", *REGION);
    log::info!("🌐 Selected endpoints:");
    log::info!("  - Astralane: {}", endpoint_config::ASTRALANE_URL.as_str());
    log::info!("  - Blockrazor: {}", endpoint_config::BLOCKRAZOR_URL.as_str());
    log::info!("  - Helius: {}", endpoint_config::HELIUS_URL.as_str());
    log::info!("  - Jito: {}", endpoint_config::JITO_URL.as_str());
    log::info!("  - NodeOne: {}", endpoint_config::NODEONE_URL.as_str());
    log::info!("  - Temporal: {}", endpoint_config::TEMPORAL_URL.as_str());
    log::info!("  - ZeroSlot: {}", endpoint_config::ZEROSLOT_URL.as_str());

    let urls = vec![
        ("Astralane", endpoint_config::ASTRALANE_URL.as_str()),
        ("Blockrazor", endpoint_config::BLOCKRAZOR_URL.as_str()),
        ("Helius", endpoint_config::HELIUS_URL.as_str()),
        ("Jito", endpoint_config::JITO_URL.as_str()),
        ("NodeOne", endpoint_config::NODEONE_URL.as_str()),
        ("Temporal", endpoint_config::TEMPORAL_URL.as_str()),
        ("ZeroSlot", endpoint_config::ZEROSLOT_URL.as_str()),
    ];

    loop {
        // 并发 ping 所有端点
        let ping_tasks: Vec<_> = urls
            .iter()
            .map(|(name, url)| {
                let client = client.clone();
                let name = *name;
                let url = *url;
                
                tokio::spawn(async move {
                    match tokio::time::timeout(
                        tokio::time::Duration::from_secs(10),
                        client.get(url).send()
                    ).await {
                        Ok(Ok(res)) => {
                            log::info!("✅ [{}] ping successful - status: {}", name, res.status());
                        }
                        Ok(Err(err)) => {
                            log::error!("❌ [{}] ping failed: {}", name, err);
                        }
                        Err(_) => {
                            log::error!("⏰ [{}] ping timeout", name);
                        }
                    }
                })
            })
            .collect();

        // 等待所有 ping 任务完成
        for task in ping_tasks {
            let _ = task.await;
        }
        
        // 使用 tokio::time::sleep 进行异步等待
        tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;
    }
}
