use std::sync::Arc;

use base64::Engine;
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
use crate::platform_clients::Region;

pub const ZEROSLOT_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("6fQaVhYZA4w3MBSXjJ81Vf6W1EDYeUPXpgVQ6UQyU1Av"),
    pubkey!("4HiwLEP2Bzqj3hM2ENxJuzhcPCdsafwiet3oGkMkuQY4"),
    pubkey!("7toBU3inhmrARGngC7z6SjyP85HgGMmCTEwGNRAcYnEK"),
    pubkey!("8mR3wB1nh4D6J9RUCugxUpc6ya8w38LPxZ3ZjcBhgzws"),
    pubkey!("6SiVU5WEwqfFapRuYCndomztEwDjvS5xgtEof3PLEGm9"),
    pubkey!("TpdxgNJBWZRL8UXF5mrEsyWxDWx9HQexA9P1eTWQ42p"),
    pubkey!("D8f3WkQu6dCF33cZxuAsrKHrGsqGP2yvAHf8mX6RXnwf"),
    pubkey!("GQPFicsy3P3NXxB5piJohoxACqTvWE9fKpLgdsMduoHE"),
    pubkey!("Ey2JEr8hDkgN8qKJGrLf2yFjRhW7rab99HVxwi5rcvJE"),
    pubkey!("4iUgjMT8q2hNZnLuhpqZ1QtiV8deFPy2ajvvjEpKKgsS"),
    pubkey!("3Rz8uD83QsU8wKvZbgWAPvCNDU6Fy8TSZTMcPm3RB6zt"),
];

pub const ZEROSLOT_ENDPOINT: &[&str] = &[
    "https://ny.0slot.trade",  // NewYork
    "https://de.0slot.trade",  // Frankfurt
    "https://ams.0slot.trade", // Amsterdam
    "https://jp.0slot.trade",  // Tokyo
    "https://la.0slot.trade",  // LosAngeles
];

pub struct ZeroSlot {
    pub endpoint: String,
    pub token: String,
    pub http_client: Arc<Client>,
}

impl ZeroSlot {
    const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => ZEROSLOT_ENDPOINT[0].to_string(),
            Region::Frankfurt => ZEROSLOT_ENDPOINT[1].to_string(),
            Region::Amsterdam => ZEROSLOT_ENDPOINT[2].to_string(),
            Region::Tokyo => ZEROSLOT_ENDPOINT[3].to_string(),
            Region::LosAngeles => ZEROSLOT_ENDPOINT[4].to_string(),
            _ => String::new(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => ZEROSLOT_ENDPOINT[0].to_string(),
            Region::Frankfurt => ZEROSLOT_ENDPOINT[1].to_string(),
            Region::Amsterdam => ZEROSLOT_ENDPOINT[2].to_string(),
            Region::Tokyo => ZEROSLOT_ENDPOINT[3].to_string(),
            Region::LosAngeles => ZEROSLOT_ENDPOINT[4].to_string(),
            _ => ZEROSLOT_ENDPOINT[0].to_string(),
        };
        let token = std::env::var("ZEROSLOT_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        ZeroSlot {
            endpoint,
            token,
            http_client,
        }
    }

    // 随机获取一个tip地址
    fn get_tip_address() -> Pubkey {
        *ZEROSLOT_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ZEROSLOT_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for ZeroSlot {
    async fn send_tx(&self, tx: &Transaction) -> Option<Signature> {
        let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
        let response = match self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("x-zeroslot-key", self.token.as_str())
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
        log::info!("zeroslot: {}", response);
        Some(tx.signatures[0])
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for ZeroSlot {
    async fn send_bundle(&self, txs: &[Transaction]) -> Option<Vec<Signature>> {
        let mut sigs = Vec::new();
        for tx in txs {
            let encode_txs =
                base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
            let response = match self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .header("x-zeroslot-key", self.token.as_str())
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
            log::info!("zeroslot: {}", response);
            sigs.push(tx.signatures[0]);
        }
        Some(sigs)
    }
}

impl crate::platform_clients::BuildTx for ZeroSlot {
    fn build_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: Option<u64>,
        nonce: Option<crate::platform_clients::NonceParam>,
        cu: Option<(u32, u64)>,
        hash: Hash,
    ) -> crate::platform_clients::TxEnvelope<'a, ZeroSlot> {
        let mut instructions = Vec::new();
        // nonce 指令
        if let Some(nonce_param) = nonce {
            match nonce_param {
                crate::platform_clients::NonceParam::Blockhash(_) => {}
                crate::platform_clients::NonceParam::NonceAccount { account, authority } => {
                    let nonce_ix =
                        solana_sdk::system_instruction::advance_nonce_account(&account, &authority);
                    instructions.push(nonce_ix);
                }
            }
        }
        // tip（必须在cu之前）
        let tip_address = Self::get_tip_address();
        let tip_amt = tip.unwrap_or(Self::MIN_TIP_AMOUNT_TX);
        let tip_ix = transfer(&signer.pubkey(), &tip_address, tip_amt);
        instructions.push(tip_ix);
        // cu
        if let Some((cu_limit, cu_price)) = cu {
            let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
            instructions.push(limit_instruction);
            let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(cu_price);
            instructions.push(price_instruction);
        }
        instructions.extend(ixs.iter().cloned());
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&signer.pubkey()),
            &[signer],
            hash,
        );
        crate::platform_clients::TxEnvelope { tx, sender: self }
    }
}

impl crate::platform_clients::BuildBundle for ZeroSlot {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, ZeroSlot> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}
