use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
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
    NonceAccount {
        account: Pubkey,
        authority: Pubkey,
        hash: Hash,
    },
}
impl NonceParam {
    fn hash(&self) -> &Hash {
        match self {
            NonceParam::Blockhash(hash) => hash,
            NonceParam::NonceAccount { hash, .. } => hash,
        }
    }
}
// 单笔交易发送 trait
#[async_trait::async_trait]
pub trait SendTx: Sync + Send {
    async fn send_tx(&self, tx: &Transaction) -> Result<Signature, String>;
}

// 批量交易发送 trait
#[async_trait::async_trait]
pub trait SendBundle: Sync + Send {
    async fn send_bundle(&self, txs: &[Transaction]) -> Result<Vec<Signature>, String>;
}

// 单笔交易组装 trait
pub trait BuildTx {
    // 需要各平台实现的方法
    fn get_tip_address(&self) -> Pubkey;
    fn get_min_tip_amount(&self) -> u64;

    // 默认实现
    fn build_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: &Option<u64>,
        nonce: &NonceParam,
        cu: &Option<(u32, u64)>,
    ) -> TxEnvelope<'a, Self>
    where
        Self: SendTx + Sync + Send + Sized,
    {
        let mut instructions = Vec::new();

        // nonce 指令
        match nonce {
            NonceParam::Blockhash(_) => {}
            NonceParam::NonceAccount {
                account, authority, ..
            } => {
                let nonce_ix =
                    solana_sdk::system_instruction::advance_nonce_account(account, authority);
                instructions.push(nonce_ix);
            }
        }

        // cu 指令（某些平台要求在 tip 之前）
        if let Some((cu_limit, cu_price)) = cu {
            let limit_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    *cu_limit,
                );
            instructions.push(limit_instruction);
            let price_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
                    *cu_price,
                );
            instructions.push(price_instruction);
        }
        // tip 转账
        if let Some(0) = tip {
            // 如果 tip 为 0，则不添加 tip 转账指令
        } else {
            let tip_address = self.get_tip_address();
            let tip_amt = tip.unwrap_or(self.get_min_tip_amount());
            let tip_ix =
                solana_sdk::system_instruction::transfer(&signer.pubkey(), &tip_address, tip_amt);
            instructions.push(tip_ix);
        }

        // 用户指令
        instructions.extend(ixs.iter().cloned());

        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&signer.pubkey()),
            &[signer],
            *nonce.hash(),
        );

        TxEnvelope { tx, sender: self }
    }
}

// 批量交易组装 trait
pub trait BuildBundle {
    fn build_bundle<'a>(&'a self, txs: &[Transaction]) -> BundleEnvelope<'a, Self>
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
pub trait TxSend: Send + Sync {
    async fn send(&self) -> Result<Signature, String>;
    fn sig(&self) -> Signature;
}

#[async_trait::async_trait]
impl<'a, T: SendTx + Sync + Send + 'a> TxSend for TxEnvelope<'a, T> {
    async fn send(&self) -> Result<Signature, String> {
        self.sender.send_tx(&self.tx).await
    }
    fn sig(&self) -> Signature {
        self.tx.signatures[0]
    }
}

// 批量 envelope
pub struct BundleEnvelope<'a, T: SendBundle + Sync + Send + 'a> {
    pub txs: Vec<Transaction>,
    pub sender: &'a T,
}

impl<'a, T: SendBundle + Sync + Send + 'a> BundleEnvelope<'a, T> {
    pub fn sigs(&self) -> Vec<Signature> {
        self.txs.iter().map(|tx| tx.signatures[0]).collect()
    }
}

#[async_trait::async_trait]
pub trait BundleSend {
    async fn send_bundle(&self) -> Result<Vec<Signature>, String>;
}

#[async_trait::async_trait]
impl<'a, T: SendBundle + Sync + Send + 'a> BundleSend for BundleEnvelope<'a, T> {
    async fn send_bundle(&self) -> Result<Vec<Signature>, String> {
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

pub async fn endpoint_keep_alive() {
    let client: Arc<reqwest::Client> = HTTP_CLIENT.clone();
    let urls = vec![
        astralane::Astralane::get_endpoint(),
        blockrazor::Blockrazor::get_endpoint(),
        helius::Helius::get_endpoint(),
        jito::Jito::get_endpoint(),
        nodeone::NodeOne::get_endpoint(),
        temporal::Temporal::get_endpoint(),
        zeroslot::ZeroSlot::get_endpoint(),
    ];
    loop {
        for url in &urls {
            let response = client.get(url).send().await;
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
