use std::fmt;
impl fmt::Display for Astralane {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Astralane")
    }
}
use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::Region;
use base64::Engine;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use solana_sdk::{
    signature::Signature,
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
    pub endpoint: String, // 只保存基础 endpoint，不拼 key
    pub auth_token: String, // 单独保存 key
    pub http_client: Arc<Client>,
}

impl Astralane {
    const MIN_TIP_AMOUNT_TX: u64 = 0_000_100_000; // 单笔交易最低 tip

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::Frankfurt => ASTRALANE_ENDPOINTS[0].to_string(),
            Region::LosAngeles => ASTRALANE_ENDPOINTS[1].to_string(),
            Region::Tokyo => ASTRALANE_ENDPOINTS[2].to_string(),
            Region::NewYork => ASTRALANE_ENDPOINTS[3].to_string(),
            Region::Amsterdam => ASTRALANE_ENDPOINTS[4].to_string(),
            _ => ASTRALANE_ENDPOINTS[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let endpoint = Self::get_endpoint().to_string();
        let auth_token = std::env::var("ASTRALANE_KEY").unwrap_or_default();
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
    async fn send_tx(&self, tx: &Transaction) -> Result<Signature, String> {
        let encode_tx = match bincode::serialize(tx) {
            Ok(bytes) => base64::prelude::BASE64_STANDARD.encode(&bytes),
            Err(e) => {
                println!("[astralane/send_tx] bincode serialize error: {}", e);
                return Err(format!("bincode serialize error: {}", e));
            }
        };
        let req_json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                encode_tx,
                {
                    "encoding": "base64",
                    "skipPreflight": true,
                },
                { "mevProtect": true }
            ],
        });
        println!("[astralane/send_tx] endpoint: {}", self.endpoint);
        println!("[astralane/send_tx] api-key(header): {}", self.auth_token);
        println!("[astralane/send_tx] request body: {}", req_json);
        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&req_json)
            .send()
            .await;
        println!("[astralane/send_tx] res: {res:?}");
        let response = match res {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    println!("[astralane/send_tx] response text error: {}", e);
                    return Err(format!("response text error: {}", e));
                }
            },
            Err(e) => {
                println!("[astralane/send_tx] send error: {}", e);
                return Err(format!("send error: {}", e));
            }
        };
        println!("[astralane/send_tx] response: {}", response);
        Ok(tx.signatures[0])
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Astralane {
    async fn send_bundle(&self, txs: &[Transaction]) -> Result<Vec<Signature>, String> {
        if txs.is_empty() {
            println!("[astralane/send_bundle] Empty transaction bundle provided");
            return Err("Empty transaction bundle provided".to_string());
        }

        let mut encoded_txs = Vec::with_capacity(txs.len());
        for tx in txs {
            match bincode::serialize(tx) {
                Ok(bytes) => encoded_txs.push(base64::prelude::BASE64_STANDARD.encode(&bytes)),
                Err(e) => {
                    println!("[astralane/send_bundle] bincode serialize error: {}", e);
                    return Err(format!("bincode serialize error: {}", e));
                }
            }
        }

        let req_json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendBundle",
            "params": [encoded_txs],
        });
        println!("[astralane/send_bundle] endpoint: {}", self.endpoint);
        println!("[astralane/send_bundle] api_key: {}", self.auth_token);
        println!("[astralane/send_bundle] request body: {}", req_json);
        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api_key", self.auth_token.as_str())
            .json(&req_json)
            .send()
            .await;
        let response = match res {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    println!("[astralane/send_bundle] response text error: {}", e);
                    return Err(format!("response text error: {}", e));
                }
            },
            Err(e) => {
                println!("[astralane/send_bundle] send error: {}", e);
                return Err(format!("send bundle error: {}", e));
            }
        };
        println!("[astralane/send_bundle] response: {}", response);
        Ok(txs.iter().map(|tx| tx.signatures[0]).collect())
    }
}

impl crate::platform_clients::BuildTx for Astralane {
    fn get_tip_address(&self) -> Pubkey {
        *ASTRALANE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| ASTRALANE_TIP_ACCOUNTS.first())
            .unwrap()
    }
    
    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    
    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for Astralane {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, Astralane> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
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
