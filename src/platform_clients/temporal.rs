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
use crate::platform_clients::Region;

pub const TEMPORAL_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq"),
    pubkey!("noz3jAjPiHuBPqiSPkkugaJDkJscPuRhYnSpbi8UvC4"),
    pubkey!("noz3str9KXfpKknefHji8L1mPgimezaiUyCHYMDv1GE"),
    pubkey!("noz6uoYCDijhu1V7cutCpwxNiSovEwLdRHPwmgCGDNo"),
    pubkey!("noz9EPNcT7WH6Sou3sr3GGjHQYVkN3DNirpbvDkv9YJ"),
    pubkey!("nozc5yT15LazbLTFVZzoNZCwjh3yUtW86LoUyqsBu4L"),
    pubkey!("nozFrhfnNGoyqwVuwPAW4aaGqempx4PU6g6D9CJMv7Z"),
    pubkey!("nozievPk7HyK1Rqy1MPJwVQ7qQg2QoJGyP71oeDwbsu"),
    pubkey!("noznbgwYnBLDHu8wcQVCEw6kDrXkPdKkydGJGNXGvL7"),
    pubkey!("nozNVWs5N8mgzuD3qigrCG2UoKxZttxzZ85pvAQVrbP"),
    pubkey!("nozpEGbwx4BcGp6pvEdAh1JoC2CQGZdU6HbNP1v2p6P"),
    pubkey!("nozrhjhkCr3zXT3BiT4WCodYCUFeQvcdUkM7MqhKqge"),
    pubkey!("nozrwQtWhEdrA6W8dkbt9gnUaMs52PdAv5byipnadq3"),
    pubkey!("nozUacTVWub3cL4mJmGCYjKZTnE9RbdY5AP46iQgbPJ"),
    pubkey!("nozWCyTPppJjRuw2fpzDhhWbW355fzosWSzrrMYB1Qk"),
    pubkey!("nozWNju6dY353eMkMqURqwQEoM3SFgEKC6psLCSfUne"),
    pubkey!("nozxNBgWohjR75vdspfxR5H9ceC7XXH99xpxhVGt3Bb"),
];

pub const TEMPORAL_ENDPOINT: &[&str] = &[
    "http://pit1.nozomi.temporal.xyz/", // AMS
    "http://tyo1.nozomi.temporal.xyz/", // Tokyo
    "http://sgp1.nozomi.temporal.xyz/", // sg
    "http://ewr1.nozomi.temporal.xyz/", // NY
    "http://ams1.nozomi.temporal.xyz/", // Amsterdam
    "http://fra2.nozomi.temporal.xyz/", //Frankfurt
];

pub struct Temporal {
    pub endpoint: String,
    pub token: String,
    pub http_client: Arc<Client>,
}

// impl
impl Temporal {
    const MIN_TIP_AMOUNT: u64 = 1_000_000;

    pub fn get_endpoint() -> String {
        match *REGION {
            // Region::Amsterdam => TEMPORAL_ENDPOINT[0].to_string(),
            Region::Tokyo => TEMPORAL_ENDPOINT[1].to_string(),
            Region::Singapore => TEMPORAL_ENDPOINT[2].to_string(),
            Region::NewYork => TEMPORAL_ENDPOINT[3].to_string(),
            Region::Amsterdam => TEMPORAL_ENDPOINT[4].to_string(),
            Region::Frankfurt => TEMPORAL_ENDPOINT[5].to_string(),
            _ => String::new(),
        }
    }

    pub fn new(token: String) -> Self {
        let endpoint = Self::get_endpoint();
        let http_client = HTTP_CLIENT.clone();
        Temporal {
            endpoint,
            token,
            http_client,
        }
    }

    // 随机获取一个tip地址
    fn get_tip_address(&self) -> Pubkey {
        *TEMPORAL_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| TEMPORAL_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl crate::platform_clients::SendTx for Temporal {
    async fn send_tx(&self, tx: &Transaction) -> Option<Signature> {
        let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
        let response = match self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("x-temporal-key", self.token.as_str())
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
        info!("temporal: {}", response);
        Some(tx.signatures[0])
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Temporal {
    async fn send_bundle(&self, txs: &[Transaction]) -> Option<Vec<Signature>> {
        let mut sigs = Vec::new();
        for tx in txs {
            let encode_txs =
                base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
            let response = match self
                .http_client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .header("x-temporal-key", self.token.as_str())
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
            info!("temporal: {}", response);
            sigs.push(tx.signatures[0]);
        }
        Some(sigs)
    }
}

impl crate::platform_clients::BuildTx for Temporal {
    fn build_tx<'a>(
        &'a self,
        mut ixs: Vec<Instruction>,
        signer: &Arc<Keypair>,
        tip: Option<u64>,
        nonce: Option<crate::platform_clients::NonceParam>,
        cu: Option<(u32, u64)>,
        hash: Hash,
    ) -> crate::platform_clients::TxEnvelope<'a, Temporal> {
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
        let tip_address = self.get_tip_address();
        let tip_amt = tip.unwrap_or(Self::MIN_TIP_AMOUNT);
        let tip_ix = transfer(&signer.pubkey(), &tip_address, tip_amt);
        instructions.push(tip_ix);
        // cu
        if let Some((cu_limit, cu_price)) = cu {
            let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
            instructions.push(limit_instruction);
            let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(cu_price);
            instructions.push(price_instruction);
        }
        instructions.append(&mut ixs);
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&signer.pubkey()),
            &[signer],
            hash,
        );
        crate::platform_clients::TxEnvelope { tx, sender: self }
    }
}

impl crate::platform_clients::BuildBundle for Temporal {
    fn build_bundle<'a>(
        &'a self,
        txs: Vec<Transaction>,
    ) -> crate::platform_clients::BundleEnvelope<'a, Temporal> {
        crate::platform_clients::BundleEnvelope { txs, sender: self }
    }
}

// use solana_system_interface::instruction;

// #[tokio::test]
// async fn test() {
//     dotenv().ok();
//     let hash = JSON_RPC_CLIENT.get_latest_blockhash().await.unwrap();
//     let ixs = instruction::transfer(&PAYER.pubkey(), &PAYER.pubkey(), 0.01.to_lamport());

//     println!("region: {}", REGION.to_string());

//     let region = Region::from(REGION.to_string());
//     println!("region: {:?}", region);

//     let temporal = Temporal::new(JSON_RPC_CLIENT.clone(), region, TEMPORAL_KEY.to_string());
//     let res = temporal.send_transaction(&mut vec![ixs], &PAYER, hash).await;

//     // let res = nonces_send_transaction(&mut vec![ixs], &PAYER, hash, &JSON_RPC_CLIENT).await;
//     println!("{:?}", res);
// }
