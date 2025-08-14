
use solana_sdk::transaction::Transaction;

use solana_sdk::{
    message::{ Message, VersionedMessage},
    transaction::VersionedTransaction,
};
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use log::info;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;

use crate::constants::HTTP_CLIENT;
pub mod astralane;
pub mod blockrazor;
pub mod helius;
pub mod jito;
pub mod nodeone;
pub mod temporal;
pub mod zeroslot;


// 通用交易枚举
pub enum SolTx {
    Legacy(Transaction),
    V0(VersionedTransaction),
}

impl SolTx {
    pub fn to_base64(&self) -> Result<String, Box<dyn std::error::Error>> {
        match self {
            SolTx::Legacy(tx) => {
                let data = bincode::serialize(tx)?;
                Ok(base64::encode(data))
            }
            SolTx::V0(v0tx) => {
                let data = bincode::serialize(v0tx)?;
                Ok(base64::encode(data))
            }
        }
    }
}




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
pub trait SendTxEncoded: Sync + Send {
    /// 发送 base64 编码后的交易
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String>;
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
        cu: &(Option<u32>, Option<u64>),
    ) -> TxEnvelope<'a, Self>
    where
        Self: SendTxEncoded + Sync + Send + Sized + Display,
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
        if let Some(cu_limit) = cu.0 {
            let limit_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
            instructions.push(limit_instruction);
        }
        if let Some(cu_price) = cu.1 {
            let price_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(cu_price);
            instructions.push(price_instruction);
        }


        // tip 转账
        if let Some(0) = tip {
            // 如果 tip 为 0，则不添加 tip 转账指令
        } else {
            let tip_address = self.get_tip_address();
            let tip_amt = tip.unwrap_or(self.get_min_tip_amount());
            info!(
                "Build Tx with tip: {} at {} tip address: {}",
                tip_amt as f64 / 1_000_000_000.0,
                self, tip_address
            );
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
pub struct TxEnvelope<'a, T: SendTxEncoded + Sync + Send + 'a> {
    pub tx: Transaction,
    pub sender: &'a T,
}

impl<'a, T: SendTxEncoded + Sync + Send + 'a> TxEnvelope<'a, T> {
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
impl<'a, T: SendTxEncoded + Sync + Send + 'a> TxSend for TxEnvelope<'a, T> {
    async fn send(&self) -> Result<Signature, String> {
        let soltx = SolTx::Legacy(self.tx.clone());
        let b64 = soltx.to_base64().map_err(|e| e.to_string())?;
        self.sender.send_tx_encoded(&b64).await;
        Ok(self.tx.signatures[0])
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
    info!("Starting endpoint keep-alive with URLs: {:?}", urls);
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
        sleep(Duration::from_secs(60)).await;
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

pub struct V0TxEnvelope<'a, T: BuildV0Tx + Sync + Send + ?Sized> {
    pub tx: VersionedTransaction,
    pub sender: &'a T,
}

impl<'a, T: BuildV0Tx + Sync + Send + ?Sized> V0TxEnvelope<'a, T> {
    pub fn tx(self) -> VersionedTransaction {
        self.tx
    }
}

#[async_trait::async_trait]
pub trait V0TxSend: Send + Sync {
    async fn send(&self) -> Result<Signature, String>;
    fn sig(&self) -> Signature;
}

#[async_trait::async_trait]
impl<'a, T: BuildV0Tx + SendTxEncoded + Sync + Send + ?Sized> V0TxSend for V0TxEnvelope<'a, T> {
    async fn send(&self) -> Result<Signature, String> {
        let soltx = SolTx::V0(self.tx.clone());
        let b64 = soltx.to_base64().map_err(|e| e.to_string())?;
        self.sender.send_tx_encoded(&b64).await;
        Ok(self.tx.signatures[0])
    }
    fn sig(&self) -> Signature {
        self.tx.signatures[0]
    }
}

pub trait BuildV0Tx {
    fn build_v0_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: &Option<u64>,
        nonce: &NonceParam,
        cu: &(Option<u32>, Option<u64>),
        address_lookup_tables: &[AddressLookupTableAccount],
    ) -> Result<V0TxEnvelope<'a, Self>, Box<dyn std::error::Error>>
    where
        Self: Sync + Send + Sized + Display + SendTxEncoded + BuildTx,
    {
        use solana_sdk::message::v0::Message as V0Message;
        use solana_sdk::transaction::VersionedTransaction;
        use solana_sdk::system_instruction;
        use solana_sdk::compute_budget::ComputeBudgetInstruction;

        let hash = *nonce.hash();
        let payer = signer.pubkey();
        let mut instructions = Vec::new();

        // nonce advance 指令
        if let NonceParam::NonceAccount { account, authority, .. } = nonce {
            let nonce_ix = system_instruction::advance_nonce_account(account, authority);
            instructions.push(nonce_ix);
        }

        // cu 指令
        if let Some(cu_limit) = cu.0 {
            let limit_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
            instructions.push(limit_instruction);
        }
        if let Some(cu_price) = cu.1 {
            let price_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(cu_price);
            instructions.push(price_instruction);
        }

        // tip 指令
        if let Some(0) = tip {
            // 不添加 tip
        } else {
            let tip_address = self.get_tip_address();
            let tip_amt = tip.unwrap_or(self.get_min_tip_amount());
            info!("Build V0Tx with tip: {} at {} tip address: {}", tip_amt as f64 / 1_000_000_000.0, self, tip_address);
            let tip_ix = system_instruction::transfer(&payer, &tip_address, tip_amt);
            instructions.push(tip_ix);
        }

        // 用户指令
        instructions.extend(ixs.iter().cloned());

        let message = V0Message::try_compile(
            &payer,
            &instructions,
            address_lookup_tables,
            hash,
        )?;
        let transaction = VersionedTransaction::try_new(
            solana_sdk::message::VersionedMessage::V0(message),
            &[signer.as_ref()],
        )?;
        Ok(V0TxEnvelope { tx: transaction, sender: self })
    }
}

// 各平台 BuildV0Tx 实现
impl BuildV0Tx for astralane::Astralane {}
impl BuildV0Tx for blockrazor::Blockrazor {}
impl BuildV0Tx for helius::Helius {}
impl BuildV0Tx for jito::Jito {}
impl BuildV0Tx for nodeone::NodeOne {}
impl BuildV0Tx for temporal::Temporal {}
impl BuildV0Tx for zeroslot::ZeroSlot {}