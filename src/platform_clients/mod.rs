use solana_compute_budget_interface::ComputeBudgetInstruction;
use base64::Engine;
use solana_sdk::message::AddressLookupTableAccount;
use solana_sdk::transaction::VersionedTransaction;
use solana_system_interface::instruction::advance_nonce_account;
use solana_system_interface::instruction::transfer;
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use utils::log_time;

use crate::constants::HTTP_CLIENT;
use log::info;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
pub mod astralane;
pub mod astralane_quic;
pub mod blockrazor;
pub mod ever_stake;
pub mod ever_stake_quic;
pub mod flash_block;
pub mod harmonic;
pub mod harmonic_proto;
pub mod helius_max;
pub mod helius_swqos;
pub mod jito;
pub mod nextblock;
pub mod nodeone;
pub mod stellium;
pub mod temporal;
pub mod zeroslot;

/// `VersionedTransaction` 的便捷扩展：序列化与签名提取
pub trait TxExt {
    /// 将交易序列化为 base64 字符串
    fn to_base64(&self) -> Result<String, Box<dyn std::error::Error>>;
    /// 获取交易签名
    fn sig(&self) -> Signature;
}

impl TxExt for VersionedTransaction {
    fn to_base64(&self) -> Result<String, Box<dyn std::error::Error>> {
        let data = bincode::serialize(self)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(data))
    }
    fn sig(&self) -> Signature {
        self.signatures[0]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 支持的平台类型
pub enum PlatformName {
    Astralane,
    Blockrazor,
    HeliusMax,
    HeliusSwqos,
    Harmonic,
    Jito,
    Nodeone,
    Temporal,
    Zeroslot,
    FlashBlock,
    Nextblock,
    Stellium,
    EverStake,
}

/// 平台枚举的字符串展示实现
impl std::fmt::Display for PlatformName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            PlatformName::Astralane => "Astralane",
            PlatformName::Blockrazor => "Blockrazor",
            PlatformName::HeliusMax => "HeliusMax",
            PlatformName::HeliusSwqos => "HeliusSwqos",
            PlatformName::Harmonic => "HarmonicBlockEngine",
            PlatformName::Jito => "Jito",
            PlatformName::Nodeone => "Nodeone",
            PlatformName::Temporal => "Temporal",
            PlatformName::Zeroslot => "Zeroslot",
            PlatformName::FlashBlock => "FlashBlock",
            PlatformName::Nextblock => "Nextblock",
            PlatformName::Stellium => "Stellium",
            PlatformName::EverStake => "EverStake",
        };
        write!(f, "{}", name)
    }
}

/// 交易小费与 CU 相关信息
#[derive(Debug, Clone)]
pub struct DetailedTx {
    pub tx: VersionedTransaction,
    pub platform: PlatformName,
    pub tip: Option<u64>,
    pub cu_limit: Option<u32>,
    pub cu_price: Option<u64>,
}

// 交易组装 trait
/// 交易哈希参数，支持普通 blockhash 和 nonce account
#[derive(Debug, Clone, Copy)]
pub enum HashParam {
    Blockhash(Hash),
    NonceAccount { account: Pubkey, authority: Pubkey, hash: Hash },
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
/// 单笔交易发送 trait，直接接收已签名的 `VersionedTransaction`，平台自行决定编码方式。
#[async_trait::async_trait]
pub trait SendTx: Sync + Send {
    /// 发送已签名的交易
    async fn send_tx(&self, tx: &VersionedTransaction) -> Result<(), String>;
}

// 批量交易发送 trait
/// 批量交易发送 trait
#[async_trait::async_trait]
pub trait SendBundle: Sync + Send {
    async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<Vec<Signature>, String>;
}

// 单笔交易组装 trait
/// 单笔交易组装 trait，各平台需实现相关方法
pub trait BuildTx {
    // 需要各平台实现的方法
    fn get_tip_address(&self) -> Pubkey;
    fn get_min_tip_amount(&self) -> u64;
    fn platform(&self) -> PlatformName;
    fn tip_recvs(&self) -> Vec<Pubkey>;

    /// 是否需要在交易中加入 SOL tip 转账指令。
    ///
    /// 默认 `true`。返回 `false` 的平台（如 Harmonic）不会写入 tip，
    /// 调用方传入任何 tip 值均被静默忽略。
    fn uses_tip_transfer(&self) -> bool {
        true
    }

    /// 是否写入 `setComputeUnitPrice`(cu_price) 指令。默认 `true`。
    ///
    /// 返回 `false` 的平台（如 HeliusMax / HeliusSwqos，走纯 tip 缓冲、不做 cu_price
    /// 竞价）会忽略调用方传入的任何 cu.price，不生成 price 指令；cu_limit(units) 仍照常写入。
    fn uses_cu_price(&self) -> bool {
        true
    }

    /// 单笔 tip 上限（lamports）。默认 `None` 表示不设上限。
    ///
    /// 返回 `Some(cap)` 的平台会把解析出的 tip 钳到 `cap` 以内（如 HeliusSwqos 上限
    /// 0.0002 SOL），`None` 的平台（如 HeliusMax）保持全额 tip 不限顶。
    fn max_tip_amount(&self) -> Option<u64> {
        None
    }
}

// 单笔 envelope
/// 单笔交易 envelope，包含交易和发送者
pub struct TxEnvelope<'a, T: SendTx + Sync + Send + 'a> {
    pub tx: DetailedTx,
    pub sender: &'a T,
}

impl<'a, T: SendTx + Sync + Send + 'a> TxEnvelope<'a, T> {
    /// 获取内部交易
    pub fn inner_tx(&self) -> &VersionedTransaction {
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

/// TxEnvelope 的发送实现
#[async_trait::async_trait]
impl<'a, T: SendTx + Sync + Send + 'a> TxSend for TxEnvelope<'a, T> {
    async fn send(&self) -> Result<Signature, String> {
        self.sender.send_tx(self.inner_tx()).await?;
        Ok(self.inner_tx().sig())
    }
    fn sig(&self) -> Signature {
        self.inner_tx().sig()
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
    Limburg,
    Lithuania,
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
            "Limburg" => Region::Limburg,
            "Lithuania" => Region::Lithuania,
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
        helius_max::HeliusMax::get_endpoint(),
        helius_swqos::HeliusSwqos::get_endpoint(),
        jito::Jito::get_endpoint(),
        nodeone::NodeOne::get_endpoint(),
        temporal::Temporal::get_endpoint(),
        zeroslot::ZeroSlot::get_endpoint(),
        flash_block::FlashBlock::get_endpoint(),
        nextblock::NextBlock::get_endpoint(),
        stellium::Stellium::get_endpoint(),
        ever_stake::EverStake::get_endpoint(),
    ];
    info!("Starting endpoint keep-alive with URLs: {:?}", urls);
    loop {
        for url in &urls {
            let start = std::time::Instant::now();
            let response = client.get(url).send().await;
            let elapsed = start.elapsed().as_millis();
            match response {
                Ok(_) => {
                    if elapsed > 10 {
                        log::warn!("{} ping successful, elapsed: {}ms", url, elapsed)
                    }
                }
                Err(err) => {
                    log::error!("{} ping failed: {}, elapsed: {}ms", url, err, elapsed);
                }
            }
        }
        sleep(Duration::from_secs(60)).await;
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
        memo: Option<Vec<&str>>,
    ) -> Result<TxEnvelope<'a, Self>, Box<dyn std::error::Error + Send + Sync>>
    where
        Self: Sync + Send + Sized + Display + SendTx + BuildTx,
    {
        use solana_sdk::message::v0::Message as V0Message;
        use solana_sdk::transaction::VersionedTransaction;
        log_time!("build transaction", {
            let hash = *nonce.hash();
            let payer = signer.pubkey();
            let mut instructions = Vec::new();

            // nonce advance 指令
            if let HashParam::NonceAccount { account, authority, .. } = nonce {
                let nonce_ix = advance_nonce_account(account, authority);
                instructions.push(nonce_ix);
            }

            // cu 指令
            if let Some(cu_limit) = cu.0 {
                let limit_instruction = ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
                instructions.push(limit_instruction);
            }
            // 平台（如 HeliusMax / HeliusSwqos）可覆写为 false，忽略 cu.price、不加 price 指令
            if self.uses_cu_price() {
                if let Some(cu_price) = cu.1 {
                    let price_instruction = ComputeBudgetInstruction::set_compute_unit_price(cu_price);
                    instructions.push(price_instruction);
                }
            }

            if self.uses_tip_transfer() {
                if let Some(0) = tip {
                    // tip = Some(0) → 不添加 tip 指令
                } else {
                    let tip_address = self.get_tip_address();
                    let mut tip_amt = tip.unwrap_or(self.get_min_tip_amount());
                    // 平台 tip 上限（如 HeliusSwqos cap 0.0002 SOL）；None 不限顶
                    if let Some(cap) = self.max_tip_amount() {
                        tip_amt = tip_amt.min(cap);
                    }
                    if tip_amt > 0 {
                        info!(
                            "Build V0Tx with tip: {}({tip_amt}lamports) at {} tip address: {}",
                            tip_amt as f64 / 1_000_000_000.0,
                            self,
                            tip_address
                        );
                        let tip_ix = transfer(&payer, &tip_address, tip_amt);
                        instructions.push(tip_ix);
                    }
                }
            }
            // uses_tip_transfer() = false 时（如 Harmonic）直接跳过，不写 tip 指令
            if let Some(memo_str) = memo {
                let memo_concat = memo_str.join("-");
                let memo_ix = solana_sdk::instruction::Instruction {
                    program_id: *crate::constants::MEMO_PROGRAM,
                    accounts: vec![],
                    data: memo_concat.as_bytes().to_vec(),
                };
                instructions.push(memo_ix);
            }

            // 用户指令
            instructions.extend(ixs.iter().cloned());

            let message = V0Message::try_compile(&payer, &instructions, address_lookup_tables, hash)?;
            let transaction =
                VersionedTransaction::try_new(solana_sdk::message::VersionedMessage::V0(message), &[signer.as_ref()])?;
            let sig = transaction.signatures[0];
            info!("  sig: {}", sig);
            Ok(TxEnvelope {
                tx: DetailedTx {
                    tx: transaction,
                    platform: self.platform(),
                    tip: *tip,
                    cu_limit: cu.0,
                    cu_price: cu.1,
                },
                sender: self,
            })
        })
    }

    /// 多签版本：`signers` 中第一个是 fee payer，其余为额外签名者。
    fn build_multisig_v0_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signers: &[&Keypair],
        tip: &Option<u64>,
        nonce: &HashParam,
        cu: &(Option<u32>, Option<u64>),
        address_lookup_tables: &[AddressLookupTableAccount],
        memo: Option<Vec<&str>>,
    ) -> Result<TxEnvelope<'a, Self>, Box<dyn std::error::Error + Send + Sync>>
    where
        Self: Sync + Send + Sized + Display + SendTx + BuildTx,
    {
        use solana_sdk::message::v0::Message as V0Message;
        use solana_sdk::transaction::VersionedTransaction;
        log_time!("build multisig transaction", {
            let hash = *nonce.hash();
            let payer = signers[0].pubkey();
            let mut instructions = Vec::new();

            if let HashParam::NonceAccount { account, authority, .. } = nonce {
                instructions.push(advance_nonce_account(account, authority));
            }
            if let Some(cu_limit) = cu.0 {
                instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(cu_limit));
            }
            // 平台（如 HeliusMax / HeliusSwqos）可覆写为 false，忽略 cu.price、不加 price 指令
            if self.uses_cu_price() {
                if let Some(cu_price) = cu.1 {
                    instructions.push(ComputeBudgetInstruction::set_compute_unit_price(cu_price));
                }
            }
            if self.uses_tip_transfer() {
                if tip != &Some(0) {
                    let tip_address = self.get_tip_address();
                    let mut tip_amt = tip.unwrap_or(self.get_min_tip_amount());
                    // 平台 tip 上限（如 HeliusSwqos cap 0.0002 SOL）；None 不限顶
                    if let Some(cap) = self.max_tip_amount() {
                        tip_amt = tip_amt.min(cap);
                    }
                    if tip_amt > 0 {
                        instructions.push(transfer(&payer, &tip_address, tip_amt));
                    }
                }
            }
            if let Some(memo_str) = memo {
                let memo_concat = memo_str.join("-");
                instructions.push(solana_sdk::instruction::Instruction {
                    program_id: *crate::constants::MEMO_PROGRAM,
                    accounts: vec![],
                    data: memo_concat.as_bytes().to_vec(),
                });
            }
            instructions.extend(ixs.iter().cloned());

            let message = V0Message::try_compile(&payer, &instructions, address_lookup_tables, hash)?;
            let transaction = VersionedTransaction::try_new(solana_sdk::message::VersionedMessage::V0(message), signers)?;
            let sig = transaction.signatures[0];
            info!("  sig: {}", sig);
            Ok(TxEnvelope {
                tx: DetailedTx {
                    tx: transaction,
                    platform: self.platform(),
                    tip: *tip,
                    cu_limit: cu.0,
                    cu_price: cu.1,
                },
                sender: self,
            })
        })
    }
}

// 各平台 BuildV0Tx 实现
impl BuildV0Tx for astralane::Astralane {}
impl BuildV0Tx for astralane_quic::client::AstralaneQuic {}
impl BuildV0Tx for blockrazor::Blockrazor {}
impl BuildV0Tx for helius_max::HeliusMax {}
impl BuildV0Tx for helius_swqos::HeliusSwqos {}
impl BuildV0Tx for harmonic::HarmonicBlockEngine {}
impl BuildV0Tx for jito::Jito {}
impl BuildV0Tx for nodeone::NodeOne {}
impl BuildV0Tx for temporal::Temporal {}
impl BuildV0Tx for zeroslot::ZeroSlot {}
impl BuildV0Tx for flash_block::FlashBlock {}
impl BuildV0Tx for nextblock::NextBlock {}
impl BuildV0Tx for stellium::Stellium {}
impl BuildV0Tx for ever_stake::EverStake {}
impl BuildV0Tx for ever_stake_quic::EverStakeQuic {}

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

// ============================================================
// Bundle Builder — 平台无关的批量交易构建器
// ============================================================

/// Bundle 发送接口（各平台实现，注入到 BundleBuilder）
#[async_trait::async_trait]
pub trait BundleSender: Send + Sync {
    async fn send_bundle(&self, txs: &[VersionedTransaction]) -> Result<Vec<Signature>, String>;
    /// 该平台的 tip 接收地址
    fn tip_address(&self) -> Pubkey;
    /// 单笔交易最大字节数（取决于传输格式：base64 / binary）
    fn max_tx_size(&self) -> usize;
    /// 是否写入 `setComputeUnitPrice`(cu_price) 指令。默认 `true`。
    /// 返回 `false`（如 HeliusMax）的 bundle 忽略调用方传入的 cu.price。
    fn uses_cu_price(&self) -> bool {
        true
    }
    /// 该平台单笔 tip 上限（lamports）。默认 `None` 表示不设上限。
    fn max_tip_amount(&self) -> Option<u64> {
        None
    }
}

/// Bundle append 失败时携带 builder，不丢已添加的交易
pub struct BundleError<T> {
    pub msg: String,
    pub builder: T,
}

impl<T> BundleError<T> {
    pub fn into_builder(self) -> T {
        self.builder
    }
}

/// 平台无关的 bundle 构建器
pub struct BundleBuilder {
    txs: Vec<VersionedTransaction>,
    sender: Box<dyn BundleSender>,
}

const MAX_TXS: usize = 5;

impl BundleBuilder {
    pub fn new(sender: Box<dyn BundleSender>) -> Self {
        Self { txs: Vec::new(), sender }
    }

    pub fn len(&self) -> usize {
        self.txs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }
    pub fn is_full(&self) -> bool {
        self.txs.len() >= MAX_TXS
    }

    /// 添加一笔交易。参数与 `build_v0_tx` 完全一致。
    /// 链式调用：`builder.append(...)?.append(...)?.send().await`
    pub fn append(
        mut self,
        ixs: &[Instruction],
        signers: &[&Keypair],
        tip: &Option<u64>,
        nonce: &HashParam,
        cu: &(Option<u32>, Option<u64>),
        address_lookup_tables: &[AddressLookupTableAccount],
        memo: Option<Vec<&str>>,
    ) -> Result<Self, BundleError<Self>> {
        use solana_sdk::message::v0::Message as V0Message;
        use solana_sdk::transaction::VersionedTransaction;

        if self.is_full() {
            return Err(BundleError {
                msg: format!("bundle full: {} >= {}", self.txs.len(), MAX_TXS),
                builder: self,
            });
        }

        let hash = *nonce.hash();
        let payer = signers.first().map(|k| k.pubkey()).unwrap_or_default();
        let mut instructions = Vec::new();

        // nonce advance
        if let HashParam::NonceAccount { account, authority, .. } = nonce {
            instructions.push(advance_nonce_account(account, authority));
        }

        // cu
        if let Some(cu_limit) = cu.0 {
            instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(cu_limit));
        }
        // 平台（如 HeliusMax）可覆写为 false，忽略 cu.price、不加 price 指令
        if self.sender.uses_cu_price() {
            if let Some(cu_price) = cu.1 {
                instructions.push(ComputeBudgetInstruction::set_compute_unit_price(cu_price));
            }
        }

        // tip：由调用方显式传入（按平台 cap 收窄）
        if let Some(tip_amt) = tip {
            let mut tip_amt = *tip_amt;
            if let Some(cap) = self.sender.max_tip_amount() {
                tip_amt = tip_amt.min(cap);
            }
            if tip_amt > 0 {
                instructions.push(transfer(&payer, &self.sender.tip_address(), tip_amt));
            }
        }

        // memo
        if let Some(memo_str) = memo {
            instructions.push(solana_sdk::instruction::Instruction {
                program_id: *crate::constants::MEMO_PROGRAM,
                accounts: vec![],
                data: memo_str.join("-").into_bytes(),
            });
        }

        instructions.extend(ixs.iter().cloned());

        let message = match V0Message::try_compile(&payer, &instructions, address_lookup_tables, hash) {
            Ok(m) => m,
            Err(e) => {
                return Err(BundleError {
                    msg: format!("compile: {e}"),
                    builder: self,
                });
            }
        };
        let transaction = match VersionedTransaction::try_new(solana_sdk::message::VersionedMessage::V0(message), signers) {
            Ok(t) => t,
            Err(e) => {
                return Err(BundleError {
                    msg: format!("sign: {e}"),
                    builder: self,
                });
            }
        };
        self.txs.push(transaction);

        // 检查最新交易的序列化大小
        let v0 = self.txs.last().unwrap();
        let size = bincode::serialize(v0).map(|b| b.len()).unwrap_or(usize::MAX);
        let max = self.sender.max_tx_size();
        if size > max {
            self.txs.pop();
            return Err(BundleError {
                msg: format!("tx too large: {size} > {max}"),
                builder: self,
            });
        }

        Ok(self)
    }

    /// 发送 bundle
    pub async fn send(self) -> Result<Vec<Signature>, String> {
        if self.txs.is_empty() {
            return Err("bundle is empty".into());
        }
        self.sender.send_bundle(&self.txs).await
    }
}
