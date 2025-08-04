use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::transaction::Transaction;

use crate::constants::{HTTP_CLIENT, endpoint_config::*};
pub mod astralane;
pub mod blockrazor;
pub mod helius;
pub mod jito;
pub mod nodeone;
pub mod temporal;
pub mod zeroslot;

// 交易组装 trait
pub enum NonceParam {
    Blockhash(Hash),
    NonceAccount { account: Pubkey, authority: Pubkey },
}

// 单笔交易发送 trait
#[async_trait::async_trait]
pub trait SendTx: Sync + Send {
    async fn send_tx(&self, tx: &Transaction) -> Option<Signature>;
}

// 批量交易发送 trait
#[async_trait::async_trait]
pub trait SendBundle: Sync + Send {
    async fn send_bundle(&self, txs: &[Transaction]) -> Option<Vec<Signature>>;
}

// 单笔交易组装 trait
pub trait BuildTx {
    fn build_tx<'a>(
        &'a self,
        ixs: Vec<Instruction>,
        signer: &Arc<Keypair>,
        tip: Option<u64>,
        nonce: Option<NonceParam>,
        cu: Option<(u32, u64)>,
        hash: Hash,
    ) -> TxEnvelope<'a, Self>
    where
        Self: SendTx + Sync + Send + Sized;
}

// 批量交易组装 trait
pub trait BuildBundle {
    fn build_bundle<'a>(&'a self, txs: Vec<Transaction>) -> BundleEnvelope<'a, Self>
    where
        Self: SendBundle + Sync + Send + Sized;
}

// 单笔 envelope
pub struct TxEnvelope<'a, T: SendTx + Sync + Send + 'a> {
    pub tx: Transaction,
    pub sender: &'a T,
}

impl<'a, T: SendTx + Sync + Send + 'a> TxEnvelope<'a, T> {
    pub fn tx(self) -> Transaction {
        self.tx
    }
}

#[async_trait::async_trait]
pub trait TxSend {
    async fn send(&self) -> Option<Signature>;
}

#[async_trait::async_trait]
impl<'a, T: SendTx + Sync + Send + 'a> TxSend for TxEnvelope<'a, T> {
    async fn send(&self) -> Option<Signature> {
        self.sender.send_tx(&self.tx).await
    }
}

// 批量 envelope
pub struct BundleEnvelope<'a, T: SendBundle + Sync + Send + 'a> {
    pub txs: Vec<Transaction>,
    pub sender: &'a T,
}

#[async_trait::async_trait]
pub trait BundleSend {
    async fn send_bundle(&self) -> Option<Vec<Signature>>;
}

#[async_trait::async_trait]
impl<'a, T: SendBundle + Sync + Send + 'a> BundleSend for BundleEnvelope<'a, T> {
    async fn send_bundle(&self) -> Option<Vec<Signature>> {
        self.sender.send_bundle(&self.txs).await
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Region {
    NewYork,
    Frankfurt,
    Amsterdam,
    London,
    SaltLakeCity,
    Tokyo,
    LosAngeles,
    Pittsburgh,
    Singapore,
    Unknown,
}

impl<T: AsRef<str>> From<T> for Region {
    fn from(value: T) -> Self {
        match value.as_ref() {
            "NewYork" => Region::NewYork,
            "Frankfurt" => Region::Frankfurt,
            "Amsterdam" => Region::Amsterdam,
            "London" => Region::London,
            "SaltLakeCity" => Region::SaltLakeCity,
            "Tokyo" => Region::Tokyo,
            "LosAngeles" => Region::LosAngeles,
            "Pittsburgh" => Region::Pittsburgh,
            "Singapore" => Region::Singapore,
            _ => Region::Unknown,
        }
    }
}

// // 记录成功上链的客户端，以及方法
// #[derive(Debug, Clone, Copy)]
// pub enum SuccessSwqos {
//     Astralane(SendMethod),
//     Blockrazor(SendMethod),
//     Helius,
//     Jito(SendMethod),
//     NodeOne,
//     Temporal,
//     ZeroSlot,
// }

// #[derive(Debug, Clone, Copy)]
// pub enum SendMethod {
//     SendTransaction,     // 发送的普通交易
//     SendBundle,          // 发送的捆绑交易
//     SendGrpcTransaction, // 通过GRPC发送的交易
// }

pub async fn endpoint_keep_alive() {
    let client: Arc<reqwest::Client> = HTTP_CLIENT.clone();

    let urls = vec![
        ASTRALANE_URL.as_str(),
        BLOCKRAZOR_URL.as_str(),
        HELIUS_URL.as_str(),
        JITO_URL.as_str(),
        NODEONE_URL.as_str(),
        TEMPORAL_URL.as_str(),
        ZEROSLOT_URL.as_str(),
    ];

    loop {
        for url in &urls {
            let response = client.get(*url).send().await;
            match response {
                Ok(_) => {
                    log::info!("{} ping successful ", url);
                }
                Err(err) => {
                    log::error!("{} ping failed: {}", err, url);
                }
            }
        }
        // 等待 60 秒
        sleep(Duration::from_secs(60));
    }
}

#[test]
fn test_region() {
    let regions = &[
        "NewYork",
        "Frankfurt",
        "Amsterdam",
        "London",
        "SaltLakeCity",
        "Tokyo",
        "LosAngeles",
        "Pittsburgh",
        "Singapore",
        "Unknown",
    ];

    for region in regions {
        println!("{}, {:?}", region, Region::from(region));
    }
}
