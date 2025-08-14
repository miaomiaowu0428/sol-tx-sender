use std::fmt;
impl fmt::Display for Jito {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Jito")
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
pub const JITO_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"),
    pubkey!("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe"),
    pubkey!("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY"),
    pubkey!("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49"),
    pubkey!("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh"),
    pubkey!("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt"),
    pubkey!("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL"),
    pubkey!("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"),
];

pub const JITO_ENDPOINTS: &[&str] = &[
    "https://ny.mainnet.block-engine.jito.wtf", // NY
    "https://frankfurt.mainnet.block-engine.jito.wtf",
    "https://amsterdam.mainnet.block-engine.jito.wtf",
    "https://london.mainnet.block-engine.jito.wtf", // london
    "https://slc.mainnet.block-engine.jito.wtf",    //
    "https://tokyo.mainnet.block-engine.jito.wtf",
    "https://singapore.mainnet.block-engine.jito.wtf",
];

pub struct Jito {
    pub endpoint: String,
    pub http_client: Arc<Client>,
}

impl Jito {
    const MIN_TIP_AMOUNT_TX: u64 = 1_000; // 单笔交易最低 tip
    const MIN_TIP_AMOUNT_BUNDLE: u64 = 10_000; // 批量交易最低 tip

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::NewYork => JITO_ENDPOINTS[0].to_string(),
            Region::Frankfurt => JITO_ENDPOINTS[1].to_string(),
            Region::Amsterdam => JITO_ENDPOINTS[2].to_string(),
            Region::London => JITO_ENDPOINTS[3].to_string(),
            Region::SaltLakeCity => JITO_ENDPOINTS[4].to_string(),
            Region::Tokyo => JITO_ENDPOINTS[5].to_string(),
            Region::Singapore => JITO_ENDPOINTS[6].to_string(),
            _ => JITO_ENDPOINTS[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let region = *crate::constants::REGION;
        let endpoint = match region {
            Region::NewYork => JITO_ENDPOINTS[0].to_string(),
            Region::Frankfurt => JITO_ENDPOINTS[1].to_string(),
            Region::Amsterdam => JITO_ENDPOINTS[2].to_string(),
            Region::London => JITO_ENDPOINTS[3].to_string(),
            Region::SaltLakeCity => JITO_ENDPOINTS[4].to_string(),
            Region::Tokyo => JITO_ENDPOINTS[5].to_string(),
            Region::Singapore => JITO_ENDPOINTS[6].to_string(),
            _ => JITO_ENDPOINTS[0].to_string(),
        };
        let http_client = HTTP_CLIENT.clone();
        Jito {
            endpoint,
            http_client,
        }
    }

    fn get_tip_address() -> Pubkey {
        *JITO_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| JITO_TIP_ACCOUNTS.first())
            .unwrap()
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for Jito {
    /// 直接接收 base64 编码后的交易数据并发送
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let request_body = match serde_json::to_string(&json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "sendTransaction",
            "params": [
                tx_base64,
                { "encoding": "base64" }
            ]
        })) {
            Ok(body) => body,
            Err(e) => return Err(format!("serde_json error: {}", e)),
        };
        let url = format!("{}/api/v1/transactions", self.endpoint);
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(request_body)
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
        log::info!("jito response: {:?}", response);
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::platform_clients::SendBundle for Jito {
    async fn send_bundle(&self, txs: &[Transaction]) -> Result<Vec<Signature>, String> {
        let mut sigs = Vec::new();
        for tx in txs {
            let encode_txs = match bincode::serialize(tx) {
                Ok(bytes) => base64::prelude::BASE64_STANDARD.encode(&bytes),
                Err(e) => return Err(format!("bincode serialize error: {}", e)),
            };
            let request_body = match serde_json::to_string(&json!({
                "id": 1,
                "jsonrpc": "2.0",
                "method": "sendBundle",
                "params": [
                    [encode_txs],
                    { "encoding": "base64" }
                ]
            })) {
                Ok(body) => body,
                Err(e) => return Err(format!("serde_json error: {}", e)),
            };
            let url = format!("{}/api/v1/bundles", self.endpoint);
            let res = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(request_body)
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
            log::info!("jito response: {:?}", response);
            sigs.push(tx.signatures[0]);
        }
        Ok(sigs)
    }
}

impl crate::platform_clients::BuildTx for Jito {
    fn get_tip_address(&self) -> Pubkey {
        Self::get_tip_address()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}

impl crate::platform_clients::BuildBundle for Jito {
    fn build_bundle<'a>(
        &'a self,
        txs: &[Transaction],
    ) -> crate::platform_clients::BundleEnvelope<'a, Jito> {
        crate::platform_clients::BundleEnvelope {
            txs: txs.to_vec(),
            sender: self,
        }
    }
}

// #[async_trait::async_trait]
// impl SwqosClientTrait for Jito {
//     async fn send_transaction(
//         &self,
//         ixs: Vec<Instruction>,
//         signer: &Arc<Keypair>,
//         feeparam: FeeParam,
//         nonce_ix: Option<Instruction>,
//     ) -> Option<(Signature, SuccessSwqos)> {
//         if self.endipoints.is_empty() {
//             return None;
//         }

//         // nonce 指令放在第一个
//         let mut instructions = Vec::new();
//         match nonce_ix {
//             Some(nonce_ix) => instructions.extend(vec![nonce_ix]),
//             None => {}
//         }

//         // tip 转账放在第二个
//         let tip_address = Self::get_tip_address();
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

//         let request_body = match serde_json::to_string(&json!({
//             "id": 1,
//             "jsonrpc": "2.0",
//             "method": "sendTransaction",
//             "params": [
//                 encode_txs,
//                 {
//                     "encoding": "base64"
//                 }
//             ]
//         })) {
//             Ok(body) => body,
//             Err(e) => {
//                 log::error!("serde_json 失败, {}", e);
//                 return None;
//             }
//         };

//         let url = format!("{}/api/v1/transactions", self.endipoints);

//         let response = match self
//             .http_client
//             .post(&url)
//             .header("Content-Type", "application/json")
//             .body(request_body)
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

//         log::info!("jito response: {:?}", response);
//         Some((
//             tx.signatures[0],
//             SuccessSwqos::Jito(SendMethod::SendTransaction),
//         ))
//     }
// }

// #[async_trait::async_trait]
// impl BundleClientTrait for Jito {
//     async fn send_bundle_transaction(
//         &self,
//         ixs: Vec<Instruction>,
//         signer: &Arc<Keypair>,
//         feeparam: FeeParam,
//         nonce_ix: Option<Instruction>,
//     ) -> Option<(Signature, SuccessSwqos)> {
//         if self.endipoints.is_empty() {
//             return None;
//         }

//         // nonce 指令放在第一个
//         let mut instrucions = Vec::new();
//         match nonce_ix {
//             Some(nonce_ix) => instrucions.extend(vec![nonce_ix]),
//             None => {}
//         }

//         // tip 转账放在第二个
//         let tip_address = Self::get_tip_address();
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
//         instrucions.push(tip_ix);

//         // cu指令放在第三位如果有
//         match feeparam.cu {
//             Some(cu) => {
//                 let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu.0);
//                 instrucions.push(limit_instruction);
//                 let price_insetuction = ComputeBudgetInstruction::set_compute_unit_price(cu.1);
//                 instrucions.push(price_insetuction);
//             }
//             None => {}
//         };
//         instrucions.extend(ixs.into_iter());

//         let tx = Transaction::new_signed_with_payer(
//             &instrucions,
//             Some(&signer.pubkey()),
//             &[signer],
//             self.hash,
//         );

//         let encode_txs = base64::prelude::BASE64_STANDARD.encode(&bincode::serialize(&tx).unwrap());
//         let request_body = match serde_json::to_string(&json!({
//             "id": 1,
//             "jsonrpc": "2.0",
//             "method": "sendBundle",
//             "params": [
//                 [encode_txs],
//                 {
//                     "encoding": "base64"
//                 }
//             ]
//         })) {
//             Ok(body) => body,
//             Err(e) => {
//                 log::error!("serde_json 失败, {}", e);
//                 return None;
//             }
//         };

//         let url = format!("{}/api/v1/bundles", self.endipoints);

//         let response = match self
//             .http_client
//             .post(&url)
//             .header("Content-Type", "application/json")
//             .body(request_body)
//             .send()
//             .await
//         {
//             Ok(res) => res.text().await.unwrap(),
//             Err(e) => {
//                 // println!("jito: {}", e);
//                 log::error!("send error: {:?}", e);
//                 return None;
//             }
//         };
//         log::info!("jito response: {:?}", response);

//         Some((tx.signatures[0], SuccessSwqos::Jito(SendMethod::SendBundle)))
//     }
// }

// // #[tokio::test]
// // async fn test() {
// //     use dotenvy::dotenv;
// //     use crate::constants::{JSON_RPC_CLIENT, PAYER, REGION};
// //     use solana_system_interface::instruction;
// //     use crate::utils::SolToLamport;

// //     dotenv().ok();
// //     let hash = JSON_RPC_CLIENT.get_latest_blockhash().await.unwrap();
// //     let ixs = instruction::transfer(&PAYER.pubkey(), &PAYER.pubkey(), 0.01.to_lamport());

// //     println!("region: {}", REGION.to_string());

// //     let region = Region::from(REGION.to_string());
// //     println!("region: {:?}", region);

// //     let temporal = Jito::new(JSON_RPC_CLIENT.clone(), region, "".to_string());
// //     let res = temporal.send_bundle_transaction(&mut vec![ixs], &PAYER, hash).await;

// //     // let res = nonces_send_transaction(&mut vec![ixs], &PAYER, hash, &JSON_RPC_CLIENT).await;
// //     println!("{:?}", res);
// // }
