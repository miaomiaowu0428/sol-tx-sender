use crate::platform_clients::Region;
use reqwest::Client;
use solana_sdk::pubkey;
use std::sync::{Arc, LazyLock};

pub static HTTP_CLIENT: LazyLock<Arc<Client>> = LazyLock::new(|| {
    Arc::new(
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
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
