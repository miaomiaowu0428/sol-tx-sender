use crate::platform_clients::Region;
use reqwest::Client;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::env;
use std::sync::{Arc, LazyLock};

pub mod api_config {
    pub const BLOCKRAZOR_KEY: &str = "";
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
pub static MEMO_PROGRAM: LazyLock<solana_sdk::pubkey::Pubkey> = LazyLock::new(|| {
    pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
});