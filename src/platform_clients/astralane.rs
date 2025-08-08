use crate::constants::HTTP_CLIENT;
use crate::platform_clients::Region;
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
use std::sync::Arc;

pub const ASTRALANE_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("astrazznxsGUhWShqgNtAdfrzP2G83DzcWVJDxwV9bF"),
    pubkey!("astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm"),
    pubkey!("astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk"),
    pubkey!("astraRVUuTHjpwEVvNBeQEgwYx9w9CFyfxjYoobCZhL"),
];

pub const ASTRALANE_ENDPOINTS: &[&str] = &[
    "http://fr.gateway.astralane.io/iris",  // Frankfurt
    "http://lax.gateway.astralane.io/iris", // San Fransisco
    "http://jp.gateway.astralane.io/iris",  // Tokyo
    "http://ny.gateway.astralane.io/iris",  // NewYork
    "http://ams.gateway.astralane.io/iris", // Amsterdam
];

pub struct Astralane {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl Astralane {
    const MIN_TIP_AMOUNT_TX: u64 = 0_000_010_000;      // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 0_003_000_000;  // 批量交易最低 tip

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::Frankfurt => ASTRALANE_ENDPOINTS[0].to_string(),
            Region::LosAngeles => ASTRALANE_ENDPOINTS[1].to_string(),
            Region::Tokyo => ASTRALANE_ENDPOINTS[2].to_string(),
            Region::NewYork => ASTRALANE_ENDPOINTS[3].to_string(),
            Region::Amsterdam => ASTRALANE_ENDPOINTS[4].to_string(),
            _ => ASTRALANE_ENDPOINTS[0].to_string(),
        };
        let auth_token = std::env::var("ASTRALANE_AUTH_TOKEN").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        Astralane {
            endpoint,
            auth_token,
            http_client,
        }
    }

    fn get_tip_address(&self) -> Pubkey {
        *ASTRALANE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ASTRALANE_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for Astralane {
    async fn send_tx(&self, tx: &Transaction) -> Option<Signature> {
        let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap());
        let response = match self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&json!({
                "transaction": encode_txs,
                "mode": "fast"
            }))
            .send()
            .await
        {
            Ok(res) => res.text().await.unwrap(),
            Err(e) => {
                println!("错误: {}", e);
                log::error!("send error: {:?}", e);
                return None;
            }
        };
        log::info!("{:?}", response);
        Some(tx.signatures[0])
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Astralane {
    async fn send_bundle(&self, txs: &[Transaction]) -> Option<Vec<Signature>> {
        if txs.is_empty() {
            log::warn!("Empty transaction bundle provided");
            return None;
        }

        // 序列化所有交易
        let encoded_txs: Vec<String> = txs
            .iter()
            .map(|tx| base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(tx).unwrap()))
            .collect();

        let response = match self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendBundle",
                "params": [encoded_txs],
            }))
            .send()
            .await
        {
            Ok(res) => res.text().await.unwrap(),
            Err(e) => {
                println!("错误: {}", e);
                log::error!("send bundle error: {:?}", e);
                return None;
            }
        };
        
        log::info!("astralane bundle response: {:?}", response);
        
        // 返回所有交易的签名
        Some(txs.iter().map(|tx| tx.signatures[0]).collect())
    }
}

impl crate::platform_clients::BuildTx for Astralane {
    fn build_tx<'a>(
        &'a self,
        ixs: Vec<Instruction>,
        signer: &Arc<Keypair>,
        tip: Option<u64>,
        nonce: Option<crate::platform_clients::NonceParam>,
        cu: Option<(u32, u64)>,
        hash: Hash,
    ) -> crate::platform_clients::TxEnvelope<'a, Astralane> {
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
        // cu（必须在tip之前）
        if let Some((cu_limit, cu_price)) = cu {
            let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
            instructions.push(limit_instruction);
            let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(cu_price);
            instructions.push(price_instruction);
        }
        // tip
        let tip_address = self.get_tip_address();
        let tip_amt = tip.unwrap_or(Self::MIN_TIP_AMOUNT_TX);
        let tip_ix = transfer(&signer.pubkey(), &tip_address, tip_amt);
        instructions.push(tip_ix);
        instructions.extend(ixs);
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&signer.pubkey()),
            &[signer],
            hash,
        );
        crate::platform_clients::TxEnvelope { tx, sender: self }
    }
}

impl crate::platform_clients::BuildBundle for Astralane {
    fn build_bundle<'a>(
        &'a self,
        txs: Vec<Transaction>,
    ) -> crate::platform_clients::BundleEnvelope<'a, Astralane> {
        crate::platform_clients::BundleEnvelope { txs, sender: self }
    }
}

// #[async_trait::async_trait]
// impl SwqosClientTrait for Astralane {
//     async fn send_transaction(
//         &self,
//         ixs: Vec<Instruction>,
//         signer: &Arc<Keypair>,
//         feeparam: FeeParam,
//         nonce_ix: Option<Instruction>,
//     ) -> Option<(Signature, SuccessSwqos)> {
//         if self.endpoints.is_empty() {
//             log::error!("endpoints 不能为空");
//             return None;
//         }

//         // nonce 指令放在第一个
//         let mut instructions = Vec::new();
//         match nonce_ix {
//             Some(nonce_ix) => instructions.extend(vec![nonce_ix]),
//             None => {}
//         }

//         // tip 转账放在第二个
//         let tip_address = self.get_tip_address();
//         let tip = match feeparam.tip {
//             Some(tip) => {
//                 if tip < Self::MIN_TIP_AMOUNT {
//                     return None;
//                 } else {
//                     tip
//                 }
//             }
//             None => Self::MIN_TIP_AMOUNT,
//         };
//         let tip_ix = transfer(&signer.pubkey(), &tip_address, tip);
//         instructions.push(tip_ix);

//         // cu指令放在第三位如果有
//         match feeparam.cu {
//             Some(cu) => {
//                 let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu.0);
//                 instructions.push(limit_instruction);
//                 let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(cu.1);
//                 instructions.push(price_instruction);
//             }
//             None => {}
//         };
//         instructions.extend(ixs.into_iter());

//         let tx = Transaction::new_signed_with_payer(
//             &instructions,
//             Some(&signer.pubkey()),
//             &[signer],
//             self.hash,
//         );
//         log::info!("{:?}", Instant::now());
//         Some((
//             tx.signatures[0],
//             SuccessSwqos::Astralane(SendMethod::SendTransaction),
//         ))
//     }
// }

// #[async_trait::async_trait]
// impl BundleClientTrait for Astralane {
//     async fn send_bundle_transaction(
//         &self,
//         ixs: Vec<Instruction>,
//         signer: &Arc<Keypair>,
//         feeparam: FeeParam,
//         nonce_ix: Option<Instruction>,
//     ) -> Option<(Signature, SuccessSwqos)> {
//         if self.endpoints.is_empty() {
//             return None;
//         }

//         // nonce 指令放在第一个
//         let mut instructions = Vec::new();
//         match nonce_ix {
//             Some(nonce_ix) => instructions.extend(vec![nonce_ix]),
//             None => {}
//         }

//         // tip 转账放在第二个
//         let tip_address = self.get_tip_address();
//         let tip = match feeparam.tip {
//             Some(tip) => {
//                 if tip < Self::MIN_TIP_AMOUNT {
//                     return None;
//                 } else {
//                     tip
//                 }
//             }
//             None => Self::MIN_TIP_AMOUNT,
//         };
//         let tip_ix = transfer(&signer.pubkey(), &tip_address, tip);
//         instructions.push(tip_ix);

//         // cu指令放在第三位如果有
//         match feeparam.cu {
//             Some(cu) => {
//                 let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu.0);
//                 instructions.push(limit_instruction);
//                 let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(cu.1);
//                 instructions.push(price_instruction);
//             }
//             None => {}
//         };
//         instructions.extend(ixs.into_iter());

//         let tx = Transaction::new_signed_with_payer(
//             &instructions,
//             Some(&signer.pubkey()),
//             &[signer],
//             self.hash,
//         );

//         let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(&tx).unwrap());

//         let response = match self
//             .http_client
//             .post(&self.endpoints)
//             .header("api_key", self.auth_token.as_str())
//             .json(&json! ({
//                 "jsonrpc": "2.0",
//                 "id": 1,
//                 "method": "sendBundle",
//                 "params": [[encode_txs]],
//             }))
//             .send()
//             .await
//         {
//             Ok(res) => res.text().await.unwrap(),
//             Err(e) => {
//                 log::error!("send error: {:?}", e);
//                 return None;
//             }
//         };
//         log::info!("astralane response: {:?}", response);
//         Some((
//             tx.signatures[0],
//             SuccessSwqos::Astralane(SendMethod::SendTransaction),
//         ))
//     }
// }
