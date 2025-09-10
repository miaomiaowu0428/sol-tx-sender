use base64::Engine;
use log::info;
use rand::seq::IndexedRandom;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

use solana_sdk::{signature::Signature, transaction::Transaction};

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::{HTTP_CLIENT, REGION};
use crate::platform_clients::{PlatformName, Region};

pub const TEMPORAL_TIP_ACCOUNTS: &[Pubkey] = &[
    // pubkey!("TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq"),
    pubkey!("noz3jAjPiHuBPqiSPkkugaJDkJscPuRhYnSpbi8UvC4"),
    // pubkey!("noz3str9KXfpKknefHji8L1mPgimezaiUyCHYMDv1GE"),
    // pubkey!("noz6uoYCDijhu1V7cutCpwxNiSovEwLdRHPwmgCGDNo"),
    // pubkey!("noz9EPNcT7WH6Sou3sr3GGjHQYVkN3DNirpbvDkv9YJ"),
    // pubkey!("nozc5yT15LazbLTFVZzoNZCwjh3yUtW86LoUyqsBu4L"),
    // pubkey!("nozFrhfnNGoyqwVuwPAW4aaGqempx4PU6g6D9CJMv7Z"),
    // pubkey!("nozievPk7HyK1Rqy1MPJwVQ7qQg2QoJGyP71oeDwbsu"),
    // pubkey!("noznbgwYnBLDHu8wcQVCEw6kDrXkPdKkydGJGNXGvL7"),
    // pubkey!("nozNVWs5N8mgzuD3qigrCG2UoKxZttxzZ85pvAQVrbP"),
    // pubkey!("nozpEGbwx4BcGp6pvEdAh1JoC2CQGZdU6HbNP1v2p6P"),
    // pubkey!("nozrhjhkCr3zXT3BiT4WCodYCUFeQvcdUkM7MqhKqge"),
    // pubkey!("nozrwQtWhEdrA6W8dkbt9gnUaMs52PdAv5byipnadq3"),
    // pubkey!("nozUacTVWub3cL4mJmGCYjKZTnE9RbdY5AP46iQgbPJ"),
    // pubkey!("nozWCyTPppJjRuw2fpzDhhWbW355fzosWSzrrMYB1Qk"),
    // pubkey!("nozWNju6dY353eMkMqURqwQEoM3SFgEKC6psLCSfUne"),
    // pubkey!("nozxNBgWohjR75vdspfxR5H9ceC7XXH99xpxhVGt3Bb"),
];

pub const TEMPORAL_ENDPOINT: &[&str] = &[
    "http://pit1.nozomi.temporal.xyz/", // AMS
    "http://tyo1.nozomi.temporal.xyz/", // Tokyo
    "http://sgp1.nozomi.temporal.xyz/", // sg
    "http://ewr1.nozomi.temporal.xyz/", // NY
    "http://ams1.nozomi.temporal.xyz/", // Amsterdam
    "http://fra2.nozomi.temporal.xyz/", //Frankfurt
];

#[derive(Clone)]
pub struct Temporal {
    pub endpoint: String,
    pub token: String,
    pub http_client: Arc<Client>,
}

// impl
impl Temporal {
    pub const MIN_TIP_AMOUNT_TX: u64 = 1_000_000; // 单笔交易最低 tip
    pub const MIN_TIP_AMOUNT_BUNDLE: u64 = 1_000_000; // 批量交易最低 tip
        pub const DEFAULT_TPS:u64 = 1;

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

    pub fn new() -> Self {
        let endpoint = Self::get_endpoint();
        let token = std::env::var("TEMPORAL_KEY").unwrap_or_default();
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
impl crate::platform_clients::SendTxEncoded for Temporal {
        
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let mut url = String::with_capacity(self.endpoint.len() + self.token.len() + 20);
        url.push_str(&self.endpoint);
        url.push_str("?c=");
        url.push_str(&self.token);
        let res = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&json! ({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendTransaction",
                "params": [
                    tx_base64,
                    {
                        "encoding": "base64",
                        "skipPreflight": true,
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
        info!("temporal: {}", response);
        Ok(())
    }
}


impl crate::platform_clients::BuildTx for Temporal {
    fn get_tip_address(&self) -> Pubkey {
        *TEMPORAL_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| TEMPORAL_TIP_ACCOUNTS.first())
            .unwrap()
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Temporal
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }

    // 使用默认实现，无需重写 build_tx
}


use std::fmt;
impl fmt::Display for Temporal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Temporal")
    }
}
