use std::sync::Arc;

use base64::Engine;
use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::system_instruction::transfer;
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{NonceParam, Region};

pub const NODEONE_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("node1PqAa3BWWzUnTHVbw8NJHC874zn9ngAkXjgWEej"),
    pubkey!("node1UzzTxAAeBTpfZkQPJXBAqixsbdth11ba1NXLBG"),
    pubkey!("node1Qm1bV4fwYnCurP8otJ9s5yrkPq7SPZ5uhj3Tsv"),
    pubkey!("node1PUber6SFmSQgvf2ECmXsHP5o3boRSGhvJyPMX1"),
    pubkey!("node1AyMbeqiVN6eoQzEAwCA6Pk826hrdqdAHR7cdJ3"),
    pubkey!("node1YtWCoTwwVYTFLfS19zquRQzYX332hs1HEuRBjC"),
];

// one one 地址
pub const NODEONE_ENDPOINT: &[&str] = &[
    "https://ny.node1.me",  // NY
    "https://fra.node1.me", // AMS
    "https://ams.node1.me", // Frankfurt
];

pub struct NodeOne {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl NodeOne {
    const MIN_TIP_AMOUNT_TX: u64 = 2_000_000; // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 2_000_000; // 批量交易最低 tip

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => NODEONE_ENDPOINT[0].to_string(),
            Region::Amsterdam => NODEONE_ENDPOINT[1].to_string(),
            Region::Frankfurt => NODEONE_ENDPOINT[2].to_string(),
            _ => String::new(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => NODEONE_ENDPOINT[0].to_string(),
            Region::Amsterdam => NODEONE_ENDPOINT[1].to_string(),
            Region::Frankfurt => NODEONE_ENDPOINT[2].to_string(),
            _ => NODEONE_ENDPOINT[0].to_string(),
        };
        let auth_token = std::env::var("NODEONE_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        NodeOne {
            endpoint,
            auth_token,
            http_client,
        }
    }

    // 随机获取一个tip地址
    fn get_tip_address() -> Pubkey {
        *NODEONE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| NODEONE_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for NodeOne {
    async fn send_tx(&self, tx: &Transaction) -> Option<Signature> {
        let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
        let response = match self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&json! ({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendTransaction",
                "params": [encode_txs],
            }))
            .send()
            .await
        {
            Ok(res) => res.text().await.unwrap(),
            Err(e) => {
                log::error!("send error: {:?}", e);
                return None;
            }
        };
        info!("node1: {}", response);
        Some(tx.signatures[0])
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for NodeOne {
    async fn send_bundle(&self, txs: &[Transaction]) -> Option<Vec<Signature>> {
        let mut sigs = Vec::new();
        for tx in txs {
            let encode_txs =
                base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
            let response = match self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .header("api-key", self.auth_token.as_str())
                .json(&json! ({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "sendTransaction",
                    "params": [encode_txs],
                }))
                .send()
                .await
            {
                Ok(res) => res.text().await.unwrap(),
                Err(e) => {
                    log::error!("send error: {:?}", e);
                    continue;
                }
            };
            info!("node1: {}", response);
            sigs.push(tx.signatures[0]);
        }
        Some(sigs)
    }
}

impl crate::platform_clients::BuildTx for NodeOne {
    fn build_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: &Option<u64>,
        nonce: &NonceParam,
        cu: &Option<(u32, u64)>,
    ) -> crate::platform_clients::TxEnvelope<'a, NodeOne> {
        let mut instructions = Vec::new();
        // nonce 指令

        if let crate::platform_clients::NonceParam::NonceAccount {
            account,
            authority,
            ..
        } = nonce
        {
            let nonce_ix =
                solana_sdk::system_instruction::advance_nonce_account(&account, &authority);
            instructions.push(nonce_ix);
        }

        // tip（必须在cu之前）
        let tip_address = Self::get_tip_address();
        let tip_amt = tip.unwrap_or(Self::MIN_TIP_AMOUNT_TX);
        let tip_ix = transfer(&signer.pubkey(), &tip_address, tip_amt);
        instructions.push(tip_ix);
        // cu
        if let Some((cu_limit, cu_price)) = cu {
            let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(*cu_limit);
            instructions.push(limit_instruction);
            let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(*cu_price);
            instructions.push(price_instruction);
        }
        instructions.extend(ixs.iter().cloned());
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&signer.pubkey()),
            &[signer],
            *nonce.hash(),
        );
        crate::platform_clients::TxEnvelope { tx, sender: self }
    }
}

impl crate::platform_clients::BuildBundle for NodeOne {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, NodeOne> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}
