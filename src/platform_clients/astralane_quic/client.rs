use anyhow::Result;
use astralane_quic_client::AstralaneQuicClient;
use log::info;
use rand::seq::IndexedRandom;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;
use std::env;
use std::fmt;
use std::sync::Arc;

use crate::constants::REGION;
use crate::platform_clients::astralane::ASTRALANE_TIP_ACCOUNTS;
use crate::platform_clients::astralane_quic::get_quic_endpoint;
use crate::platform_clients::{PlatformName, Region, SendTx};

#[derive(Clone)]
pub struct AstralaneQuic {
    client: Arc<AstralaneQuicClient>,
    endpoint: String,
}

impl AstralaneQuic {
    pub const MIN_TIP_AMOUNT_TX: u64 = 100_000; // 单笔交易最低 tip (100,000 lamports)

    pub fn get_endpoint() -> String {
        get_quic_endpoint(&REGION).to_string()
    }

    pub async fn new() -> Result<Self, String> {
        let endpoint = Self::get_endpoint();
        let api_key = env::var("ASTRALANE_KEY").map_err(|e| format!("ASTRALANE_KEY env var required: {}", e))?;

        let client = AstralaneQuicClient::connect(&endpoint, &api_key)
            .await
            .map_err(|e| format!("Failed to connect to Astralane QUIC: {}", e))?;

        Ok(Self {
            client: Arc::new(client),
            endpoint,
        })
    }

    pub async fn init_with(key: impl Into<String>, region: Region) -> Result<Self, String> {
        let endpoint = get_quic_endpoint(&region).to_string();
        let api_key = key.into();

        let client = AstralaneQuicClient::connect(&endpoint, &api_key)
            .await
            .map_err(|e| format!("Failed to connect to Astralane QUIC: {}", e))?;

        Ok(Self {
            client: Arc::new(client),
            endpoint,
        })
    }

}

impl fmt::Display for AstralaneQuic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AstralaneQuic({})", self.endpoint)
    }
}

#[async_trait::async_trait]
impl SendTx for AstralaneQuic {
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String> {
        // 直接序列化为字节，QUIC 发送，无需 base64
        let tx_bytes = crate::platform_clients::serialize_transaction_wire(tx)?;

        self.client
            .send_transaction(&tx_bytes)
            .await
            .map_err(|e| format!("Astralane QUIC send error: {}", e))?;

        let sig = tx.signatures[0];
        info!("[AstralaneQuic] Sent transaction signature: {}", sig);

        Ok(())
    }
}

impl crate::platform_clients::BuildTx for AstralaneQuic {
    fn platform(&self) -> PlatformName {
        PlatformName::Astralane
    }

    fn get_tip_address(&self) -> Pubkey {
        // 随机选择一个 tip 账户
        *ASTRALANE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ASTRALANE_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    fn tip_recvs(&self) -> Vec<Pubkey> {
        ASTRALANE_TIP_ACCOUNTS.to_vec()
    }
}
