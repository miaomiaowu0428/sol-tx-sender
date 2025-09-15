use solana_sdk::transaction::Transaction;

use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::transaction::VersionedTransaction;
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
pub mod flash_block;
pub mod helius;
pub mod jito;
pub mod nextblock;
pub mod nodeone;
pub mod temporal;
pub mod zeroslot;

// 通用交易枚举
/// 通用交易类型，兼容 Legacy 和 V0 版本
#[derive(Clone, Debug)]
pub enum SolTx {
    Legacy(Transaction),
    V0(VersionedTransaction),
}

impl SolTx {
    /// 将交易序列化为 base64 字符串
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
    pub fn sig(&self) -> Signature {
        match self {
            SolTx::Legacy(transaction) => transaction.signatures[0],
            SolTx::V0(versioned_transaction) => versioned_transaction.signatures[0],
        }
    }
}

// 自定义 SolTx 的 Serialize 实现，只序列化内部内容，不包含变体信息
impl serde::Serialize for SolTx {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SolTx::Legacy(tx) => tx.serialize(serializer),
            SolTx::V0(v0tx) => v0tx.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 支持的平台类型
pub enum PlatformName {
    Astralane,
    Blockrazor,
    Helius,
    Jito,
    Nodeone,
    Temporal,
    Zeroslot,
    FlashBlock,
    Nextblock,
}

/// 平台枚举的字符串展示实现
impl std::fmt::Display for PlatformName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            PlatformName::Astralane => "Astralane",
            PlatformName::Blockrazor => "Blockrazor",
            PlatformName::Helius => "Helius",
            PlatformName::Jito => "Jito",
            PlatformName::Nodeone => "Nodeone",
            PlatformName::Temporal => "Temporal",
            PlatformName::Zeroslot => "Zeroslot",
            PlatformName::FlashBlock => "FlashBlock",
            PlatformName::Nextblock => "Nextblock",
        };
        write!(f, "{}", name)
    }
}

/// 交易小费与 CU 相关信息
#[derive(Debug, Clone)]
pub struct DetailedTx {
    pub tx: SolTx,
    pub platform: PlatformName,
    pub tip: Option<u64>,
    pub cu_limit: Option<u32>,
    pub cu_price: Option<u64>,
}

// 交易组装 trait
/// 交易哈希参数，支持普通 blockhash 和 nonce account
pub enum HashParam {
    Blockhash(Hash),
    NonceAccount {
        account: Pubkey,
        authority: Pubkey,
        hash: Hash,
    },
}
impl HashParam {
    /// 获取当前哈希值
    fn hash(&self) -> &Hash {
        match self {
            HashParam::Blockhash(hash) => hash,
            HashParam::NonceAccount { hash, .. } => hash,
        }
    }
}
// 单笔交易发送 trait
/// 单笔交易发送 trait，发送 base64 编码后的交易
#[async_trait::async_trait]
pub trait SendTxEncoded: Sync + Send {
    /// 发送 base64 编码后的交易
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String>;
}

// 批量交易发送 trait
/// 批量交易发送 trait
#[async_trait::async_trait]
pub trait SendBundle: Sync + Send {
    async fn send_bundle(&self, txs: &[SolTx]) -> Result<Vec<Signature>, String>;
}

// 单笔交易组装 trait
/// 单笔交易组装 trait，各平台需实现相关方法
pub trait BuildTx {
    // 需要各平台实现的方法
    fn get_tip_address(&self) -> Pubkey;
    fn get_min_tip_amount(&self) -> u64;
    fn platform(&self) -> PlatformName;

    // 默认实现
    /// 默认交易组装实现，支持 tip、cu、nonce 等参数
    fn build_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: &Option<u64>,
        nonce: &HashParam,
        cu: &(Option<u32>, Option<u64>),
    ) -> TxEnvelope<'a, Self>
    where
        Self: SendTxEncoded + Sync + Send + Sized + Display,
    {
        let mut instructions = Vec::new();

        // nonce 指令
        match nonce {
            HashParam::Blockhash(_) => {}
            HashParam::NonceAccount {
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
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    cu_limit,
                );
            instructions.push(limit_instruction);
        }
        if let Some(cu_price) = cu.1 {
            let price_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
                    cu_price,
                );
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
                self,
                tip_address
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

        TxEnvelope {
            tx: DetailedTx {
                tx: SolTx::Legacy(tx),
                platform: self.platform(),
                tip: *tip,
                cu_limit: cu.0,
                cu_price: cu.1,
            },
            sender: self,
        }
    }
}

// 批量交易组装 trait
/// 批量交易组装 trait
pub trait BuildBundle {
    fn build_bundle<'a>(&'a self, txs: &[SolTx]) -> BundleEnvelope<'a, Self>
    where
        Self: SendBundle + Sync + Send + Sized;
}

// 单笔 envelope
/// 单笔交易 envelope，兼容 Legacy/V0，包含 SolTx 和发送者
pub struct TxEnvelope<'a, T: SendTxEncoded + Sync + Send + 'a> {
    pub tx: DetailedTx,
    pub sender: &'a T,
}

impl<'a, T: SendTxEncoded + Sync + Send + 'a> TxEnvelope<'a, T> {
    /// 获取内部 SolTx
    pub fn inner_tx(&self) -> &SolTx {
        &self.tx.tx
    }
    /// 获取签名
    pub fn sig(&self) -> Signature {
        self.inner_tx().sig()
    }
}

/// 单笔交易发送 trait，异步发送并返回签名
#[async_trait::async_trait]
pub trait TxSend: Send + Sync {
    async fn send(&self) -> Result<Signature, String>;
    fn sig(&self) -> Signature;
}

/// TxEnvelope 的发送实现，兼容 Legacy/V0
#[async_trait::async_trait]
impl<'a, T: SendTxEncoded + Sync + Send + 'a> TxSend for TxEnvelope<'a, T> {
    async fn send(&self) -> Result<Signature, String> {
        let b64 = self.inner_tx().to_base64().map_err(|e| e.to_string())?;
        let _ = self.sender.send_tx_encoded(&b64).await;
        Ok(self.inner_tx().sig())
    }
    fn sig(&self) -> Signature {
        self.inner_tx().sig()
    }
}

// 批量 envelope
/// 批量交易 envelope，包含多笔交易和发送者
pub struct BundleEnvelope<'a, T: SendBundle + Sync + Send + 'a> {
    pub txs: Vec<SolTx>,
    pub sender: &'a T,
}

impl<'a, T: SendBundle + Sync + Send + 'a> BundleEnvelope<'a, T> {
    /// 获取所有交易的签名
    pub fn sigs(&self) -> Vec<Signature> {
        self.txs.iter().map(|tx| tx.sig()).collect()
    }
}

/// 批量交易发送 trait
#[async_trait::async_trait]
pub trait BundleSend {
    async fn send_bundle(&self) -> Result<Vec<Signature>, String>;
}

/// BundleEnvelope 的批量发送实现
#[async_trait::async_trait]
impl<'a, T: SendBundle + Sync + Send + 'a> BundleSend for BundleEnvelope<'a, T> {
    async fn send_bundle(&self) -> Result<Vec<Signature>, String> {
        self.sender.send_bundle(&self.txs).await
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
/// 区域枚举
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

/// 字符串转 Region 枚举实现
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

/// 各平台 endpoint 保活定时任务
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
        flash_block::FlashBlock::get_endpoint(),
        nextblock::NextBlock::get_endpoint(),
    ];
    info!("Starting endpoint keep-alive with URLs: {:?}", urls);
    loop {
        for url in &urls {
            let start = std::time::Instant::now();
            let response = client.get(url).send().await;
            let elapsed = start.elapsed().as_millis();
            match response {
                Ok(_) => {
                    log::info!("{} ping successful, elapsed: {}ms", url, elapsed);
                }
                Err(err) => {
                    log::error!("{} ping failed: {}, elapsed: {}ms", url, err, elapsed);
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

/// V0 交易组装 trait，直接使用默认实现即可
pub trait BuildV0Tx {
    /// 默认 V0 交易组装实现，支持 tip、cu、nonce、lookup table 等参数
    fn build_v0_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: &Option<u64>,
        nonce: &HashParam,
        cu: &(Option<u32>, Option<u64>),
        address_lookup_tables: &[AddressLookupTableAccount],
    ) -> Result<TxEnvelope<'a, Self>, Box<dyn std::error::Error>>
    where
        Self: Sync + Send + Sized + Display + SendTxEncoded + BuildTx,
    {
        use solana_sdk::message::v0::Message as V0Message;
        use solana_sdk::system_instruction;
        use solana_sdk::transaction::VersionedTransaction;

        let hash = *nonce.hash();
        let payer = signer.pubkey();
        let mut instructions = Vec::new();

        // nonce advance 指令
        if let HashParam::NonceAccount {
            account, authority, ..
        } = nonce
        {
            let nonce_ix = system_instruction::advance_nonce_account(account, authority);
            instructions.push(nonce_ix);
        }

        // cu 指令
        if let Some(cu_limit) = cu.0 {
            let limit_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    cu_limit,
                );
            instructions.push(limit_instruction);
        }
        if let Some(cu_price) = cu.1 {
            let price_instruction =
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
                    cu_price,
                );
            instructions.push(price_instruction);
        }

        // tip 指令
        if let Some(0) = tip {
            // 不添加 tip
        } else {
            let tip_address = self.get_tip_address();
            let tip_amt = tip.unwrap_or(self.get_min_tip_amount());
            info!(
                "Build V0Tx with tip: {}({tip_amt}lamports) at {} tip address: {}",
                tip_amt as f64 / 1_000_000_000.0,
                self,
                tip_address
            );
            let tip_ix = system_instruction::transfer(&payer, &tip_address, tip_amt);
            instructions.push(tip_ix);
        }

        // 用户指令
        instructions.extend(ixs.iter().cloned());

        let message = V0Message::try_compile(&payer, &instructions, address_lookup_tables, hash)?;
        let transaction = VersionedTransaction::try_new(
            solana_sdk::message::VersionedMessage::V0(message),
            &[signer.as_ref()],
        )?;
        Ok(TxEnvelope {
            tx: DetailedTx {
                tx: SolTx::V0(transaction),
                platform: self.platform(),
                tip: *tip,
                cu_limit: cu.0,
                cu_price: cu.1,
            },
            sender: self,
        })
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
impl BuildV0Tx for flash_block::FlashBlock {}
impl BuildV0Tx for nextblock::NextBlock {}
