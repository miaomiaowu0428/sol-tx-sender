use std::fmt;
impl fmt::Display for EverStake {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Astralane")
    }
}

use base64::Engine;
use rand::seq::IndexedRandom;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

use solana_sdk::{pubkey, pubkey::Pubkey};

use crate::constants::REGION;
use crate::platform_clients::{PlatformName, Region, TxSend};

pub const EVER_STAKE_TIP_ACCOUNTS: &[Pubkey] = &[
    pubkey!("J4cL8c22KNLHwheuWxK1SCYBWASWPGhEi6xvcGyf6o3S"),
    pubkey!("EzuhsszPxRUHBwGPXtKoqCB58EiTJ1QiYA2XrhbUEFbr"),
    pubkey!("7wsUm2VDopGDFyXkyhmgUh9V15QkEvnyqbgUPcagLcw2"),
    pubkey!("Cy3WAM9NdjFG3kXCxXmD17WmtJMBKVpoBXabkSm88Xdt"),
    pubkey!("BEEya88mme6JJ4rgshBR23eiDHmygUii9opUHE3qxnqK"),
    pubkey!("Gq21dPAGVuuZucqBQeCkfbbqoEowL1t88igZekJ93CRu"),
    pubkey!("79HFWkNoPhotXuFYi1ksuK5hE7AUnKasafP6c71hS9sM"),
    pubkey!("Cp4pCm5JjDaZ4gXB8eSjNJvQ8eg7uK6awgjveofrSATz"),
    pubkey!("DMHQ51qK2wChtDEUED54cqzbSLMLGvTygQCv5uLTUmZP"),
    pubkey!("GDnz7cAA7hKEFmDyrk6mz3drybHWc3Gn14y9LCsvvtjE"),
];

pub const EVER_STAKE_ENDPOINTS: &[&str] = &[
    "http://main-swqos.everstake.one",
    "http://fra-swqos.everstake.one", // San Fransisco
    "http://ny-swqos.everstake.one",  // Tokyo
    "http://tyo-swqos.everstake.one", // NewYork
    "http://ams-swqos.everstake.one", // Amsterdam
];

#[derive(Clone)]
pub struct EverStake {
    pub json_rpc_client: Arc<RpcClient>,
}

impl EverStake {
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_000_500_000; // 单笔交易最低 tip
    pub const DEFAULT_TPS: u64 = 5;

    pub fn get_endpoint() -> String {
        match *REGION {
            Region::Frankfurt => EVER_STAKE_ENDPOINTS[1].to_string(),
            Region::Tokyo => EVER_STAKE_ENDPOINTS[3].to_string(),
            Region::NewYork => EVER_STAKE_ENDPOINTS[2].to_string(),
            Region::Amsterdam => EVER_STAKE_ENDPOINTS[4].to_string(),
            _ => EVER_STAKE_ENDPOINTS[0].to_string(),
        }
    }

    pub fn new() -> Self {
        let endpoint = Self::get_endpoint().to_string();
        EverStake {
            json_rpc_client: Arc::new(RpcClient::new(endpoint)),
        }
    }
}


#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for EverStake {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let bytes = base64::prelude::BASE64_STANDARD
            .decode(tx_base64)
            .map_err(|e| e.to_string())?;

        let tx: solana_sdk::transaction::Transaction =
            bincode::deserialize(&bytes).map_err(|e| e.to_string())?;

        match self.json_rpc_client.send_transaction(&tx).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Everstake send error: {}", e)),
        }
    }
}

impl crate::platform_clients::BuildTx for EverStake {
    fn platform(&self) -> PlatformName {
        PlatformName::EverStake
    }
    fn get_tip_address(&self) -> Pubkey {
        *EVER_STAKE_TIP_ACCOUNTS
            .choose(&mut rand::rng())
            .or_else(|| EVER_STAKE_TIP_ACCOUNTS.first())
            .unwrap()
    }

    fn get_min_tip_amount(&self) -> u64 {
        Self::MIN_TIP_AMOUNT_TX
    }
}
