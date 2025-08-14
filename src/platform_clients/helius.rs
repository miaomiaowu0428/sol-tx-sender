use std::fmt;
impl fmt::Display for Helius {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Helius")
    }
}
use base64::Engine;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::{signature::Signature, transaction::Transaction};

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::Region;

// helius 小费地址
pub const HELIUS_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE"),
    pubkey!("D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ"),
    pubkey!("9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta"),
    pubkey!("5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn"),
    pubkey!("2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD"),
    pubkey!("2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ"),
    pubkey!("wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF"),
    pubkey!("3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT"),
    pubkey!("4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey"),
    pubkey!("4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or"),
];

// helius 地址
pub const HELIUS_ENDPOINT: &[&str] = &[
    "http://ewr-sender.helius-rpc.com/fast", // NY
    "http://ams-sender.helius-rpc.com/fast", // Amsterdam
    "http://fra-sender.helius-rpc.com/fast", // Frankfurt
    "http://lon-sender.helius-rpc.com/fast", // London
    "http://slc-sender.helius-rpc.com/fast", // Salt Lake City
    "http://tyo-sender.helius-rpc.com/fast", // Tokyo
    "http://sg-sender.helius-rpc.com/fast",  // Singapore
];

pub struct Helius {
    pub endpoint: String,
    pub auth_token: String,
    pub http_client: Arc<Client>,
}

impl Helius {
    const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip  
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => HELIUS_ENDPOINT[0].to_string(),
            Region::Amsterdam => HELIUS_ENDPOINT[1].to_string(),
            Region::Frankfurt => HELIUS_ENDPOINT[2].to_string(),
            Region::London => HELIUS_ENDPOINT[3].to_string(),
            Region::SaltLakeCity => HELIUS_ENDPOINT[4].to_string(),
            Region::Tokyo => HELIUS_ENDPOINT[5].to_string(),
            Region::Singapore => HELIUS_ENDPOINT[6].to_string(),
            _ => HELIUS_ENDPOINT[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => HELIUS_ENDPOINT[0].to_string(),
            Region::Amsterdam => HELIUS_ENDPOINT[1].to_string(),
            Region::Frankfurt => HELIUS_ENDPOINT[2].to_string(),
            Region::London => HELIUS_ENDPOINT[3].to_string(),
            Region::SaltLakeCity => HELIUS_ENDPOINT[4].to_string(),
            Region::Tokyo => HELIUS_ENDPOINT[5].to_string(),
            Region::Singapore => HELIUS_ENDPOINT[6].to_string(),
            _ => HELIUS_ENDPOINT[0].to_string(),
        };
        let auth_token = std::env::var("HELIUS_KEY").unwrap_or_default();
        let http_client = HTTP_CLIENT.clone();
        Helius {
            endpoint,
            auth_token,
            http_client,
        }
    }

    fn get_tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTx for Helius {
    async fn send_tx(&self, tx: &Transaction) -> Result<Signature, String> {
        let encode_txs = match bincode::serialize(tx) {
            Ok(bytes) => base64::prelude::BASE64_STANDARD.encode(&bytes),
            Err(e) => return Err(format!("bincode serialize error: {}", e)),
        };
        let res = self
            .http_client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("api-key", self.auth_token.as_str())
            .json(&json!({
                "id": 1,
                "jsonrpc": "2.0",
                "method": "sendTransaction",
                "params": [
                    encode_txs,
                    {
                        "encoding": "base64",
                        "skipPreflight": true,
                        "maxRetries": 0,
                    }
                ],
            }))
            .send()
            .await;
        let response = match res {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => return Err(format!("response text error: {}", e)),
            },
            Err(e) => {
                log::error!("send error: {:?}", e);
                return Err(format!("send error: {}", e));
            }
        };
        log::info!("{:?}", response);
        Ok(tx.signatures[0])
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Helius {
    async fn send_bundle(&self, _txs: &[Transaction]) -> Result<Vec<Signature>, String> {
        Err("Helius 暂不支持批量交易".to_string())
    }
}

impl crate::platform_clients::BuildTx for Helius {
    fn get_tip_address(&self) -> Pubkey {
        *HELIUS_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| HELIUS_TIP_ACCOUNTS.first())
            .unwrap()
    }
    
    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
    
    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for Helius {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, Helius> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}

// #[async_trait::async_trait]
// impl SwqosClientTrait for Helius {
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

//         let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(&tx).unwrap());

//         let response = match self
//             .http_client
//             .post(&self.endpoints)
//             .header("Content-Type", "application/json")
//             .header("api-key", self.auth_token.as_str())
//             .json(&json! ({
//                 "id": 1,
//                 "jsonrpc": "2.0",
//                 "method": "sendTransaction",
//                 "params": [
//                     encode_txs,                      // Must include both tip and priority fee instructions
//                     {
//                         "encoding": "base64",
//                         "skipPreflight": true,       // Required: must be true
//                         "maxReties": 0,
//                     }
//                 ],
//             }))
//             .send()
//             .await
//         {
//             Ok(res) => res.text().await.unwrap(),
//             Err(e) => {
//                 // println!("node1: {}", e);
//                 log::error!("send error: {:?}", e);
//                 return None;
//             }
//         };

//         log::info!("{:?}", response);
//         return Some((tx.signatures[0], SuccessSwqos::Helius));
//     }
// }
