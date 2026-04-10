use anyhow::Result;
use astralane_quic_client::AstralaneQuicClient;
use base64::Engine;
use log::info;
use rand::seq::IndexedRandom;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::Transaction;
use std::env;
use std::fmt;

use crate::constants::REGION;
use crate::platform_clients::astralane::ASTRALANE_TIP_ACCOUNTS;
use crate::platform_clients::astralane_quic::get_quic_endpoint;
use crate::platform_clients::{BuildTx, PlatformName, Region, SendTxEncoded};

pub struct AstralaneQuic {
    client: AstralaneQuicClient,
    endpoint: String,
    api_key: String,
}

impl AstralaneQuic {
    pub const MIN_TIP_AMOUNT_TX: u64 = 100_000; // 单笔交易最低 tip (100,000 lamports)
    pub const DEFAULT_TPS: u64 = 5;

    pub fn get_endpoint() -> String {
        get_quic_endpoint(&REGION).to_string()
    }

    pub async fn new() -> Result<Self, String> {
        let endpoint = Self::get_endpoint();
        let api_key = env::var("ASTRALANE_KEY")
            .map_err(|e| format!("ASTRALANE_KEY env var required: {}", e))?;

        let client = AstralaneQuicClient::connect(&endpoint, &api_key)
            .await
            .map_err(|e| format!("Failed to connect to Astralane QUIC: {}", e))?;

        Ok(Self {
            client,
            endpoint,
            api_key,
        })
    }

    pub async fn init_with(key: impl Into<String>, region: Region) -> Result<Self, String> {
        let endpoint = get_quic_endpoint(&region).to_string();
        let api_key = key.into();

        let client = AstralaneQuicClient::connect(&endpoint, &api_key)
            .await
            .map_err(|e| format!("Failed to connect to Astralane QUIC: {}", e))?;

        Ok(Self {
            client,
            endpoint,
            api_key,
        })
    }

    // Sync version for convenience
    pub async fn send_transaction(&self, tx: &Transaction) -> Result<Signature, String> {
        let tx_bytes = bincode::serialize(tx)
            .map_err(|e| format!("Failed to serialize transaction: {}", e))?;

        self.client
            .send_transaction(&tx_bytes)
            .await
            .map_err(|e| format!("Failed to send QUIC transaction: {}", e))?;

        let sig = tx.signatures[0];
        Ok(sig)
    }
}

impl fmt::Display for AstralaneQuic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AstralaneQuic({})", self.endpoint)
    }
}

#[async_trait::async_trait]
impl SendTxEncoded for AstralaneQuic {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        // Decode base64 to bytes
        let tx_bytes = base64::prelude::BASE64_STANDARD
            .decode(tx_base64)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        // Send via QUIC
        self.client
            .send_transaction(&tx_bytes)
            .await
            .map_err(|e| format!("Astralane QUIC send error: {}", e))?;

        // Decode again to get signature for logging
        let tx: Transaction = bincode::deserialize(&tx_bytes)
            .map_err(|e| format!("Failed to deserialize transaction: {}", e))?;
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
