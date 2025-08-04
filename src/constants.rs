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

    pub static BLOCKRAZOR_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("BLOCKRAZOR_URL").unwrap_or_else(|_| "https://api.blockrazor.xyz".to_string())
    });
    pub static HELIUS_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("HELIUS_URL").unwrap_or_else(|_| "https://api.helius-rpc.com".to_string())
    });
    pub static JITO_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("JITO_URL").unwrap_or_else(|_| "https://api.jito.wtf".to_string())
    });
    pub static NODEONE_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("NODEONE_URL").unwrap_or_else(|_| "https://api.nodeone.io".to_string())
    });
    pub static TEMPORAL_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("TEMPORAL_URL").unwrap_or_else(|_| "https://api.temporal.xyz".to_string())
    });
    pub static ZEROSLOT_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("ZEROSLOT_URL").unwrap_or_else(|_| "https://api.zeroslot.com".to_string())
    });
    pub static ASTRALANE_URL: LazyLock<String> = LazyLock::new(|| {
        std::env::var("ASTRALANE_URL").unwrap_or_else(|_| "https://api.astralane.com".to_string())
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
