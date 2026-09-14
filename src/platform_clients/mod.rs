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

/// V1 交易组装（SIMD-0385 / SIMD-0296）。
///
/// # 与 V0 的差异
///
/// | 维度 | V0 (`BuildV0Tx`) | V1（本 trait） |
/// |---|---|---|
/// | 大小上限 | 1232 字节 | **4096 字节** |
/// | 地址查找表 | 支持（`address_lookup_tables`） | **不支持**，账户必须内联 |
/// | compute budget | 写成 `ComputeBudgetProgram` 指令 | **写进消息 `config`** |
/// | 账户上限 | 64（经 ALT 可达） | 64（内联即可达到） |
///
/// # 参数说明
///
/// - `config`：交易级资源限制。**取代了 V0 的 `cu` 参数**——V1 不再需要
///   构造 `SetComputeUnitLimit` / `SetComputeUnitPrice` 指令，这两个值
///   直接写进消息的 `config` 字段，网络只需定长读取即可排序。
/// - 没有 `address_lookup_tables`：V1 不支持 ALT，所以账户全部内联。
///   这也是 V1 能用 4096 字节装下 64 个账户的原因（64 × 32 = 2048 字节）。
///
/// # 注意
///
/// `config` 里的 `None` 语义是**取 0**（不是"无限制"）——这是 V1 与 V0 的
/// 重要差异。V0 不写 ComputeBudget 指令表示"用运行时默认值"，而 V1 的
/// `None` 表示按 0 处理。所以调用方需要显式给出想要的值。
pub trait BuildV1Tx {
    /// 单签名者：用 `v1::Message::try_compile_with_config` 组装并签名。
    ///
    /// `config` 取代了 V0 的 `cu` 参数；不再有 `address_lookup_tables`。
    fn build_v1_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signer: &Arc<Keypair>,
        tip: &Option<u64>,
        nonce: &HashParam,
        config: V1TxConfig,
        memo: Option<Vec<&str>>,
    ) -> Result<TxEnvelope<'a, Self>, Box<dyn std::error::Error + Send + Sync>>
    where
        Self: Sync + Send + Sized + Display + SendTx + BuildTx,
    {
        use solana_sdk::message::v1::Message as V1Message;
        use solana_sdk::message::VersionedMessage;
        log_time!("build V1 transaction", {
            let hash = *nonce.hash();
            let payer = signer.pubkey();
            let mut instructions = Vec::new();

            // nonce advance 仍是普通指令（V1 只把 compute budget 搬进了 config）。
            if let HashParam::NonceAccount { account, authority, .. } = nonce {
                instructions.push(advance_nonce_account(account, authority));
            }

            // 注意：这里**不再**添加 ComputeBudget 指令 —— cu limit / price
            // 通过 `config` 传给消息本身。

            if self.uses_tip_transfer() {
                if let Some(0) = tip {
                    // tip = Some(0) → 不添加 tip 指令
                } else {
                    let tip_address = self.get_tip_address();
                    let mut tip_amt = tip.unwrap_or(self.get_min_tip_amount());
                    if let Some(cap) = self.max_tip_amount() {
                        tip_amt = tip_amt.min(cap);
                    }
                    if tip_amt > 0 {
                        info!(
                            "Build V1Tx with tip: {}({tip_amt}lamports) at {} tip address: {}",
                            tip_amt as f64 / 1_000_000_000.0,
                            self,
                            tip_address
                        );
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

            // 用户指令
            instructions.extend(ixs.iter().cloned());

            let message =
                V1Message::try_compile_with_config(&payer, &instructions, hash, config.to_transaction_config())?;
            let transaction =
                VersionedTransaction::try_new(VersionedMessage::V1(message), &[signer.as_ref()])?;
            let sig = transaction.signatures[0];
            info!("  sig: {}", sig);
            Ok(TxEnvelope {
                tx: DetailedTx {
                    tx: transaction,
                    platform: self.platform(),
                    tip: *tip,
                    cu_limit: config.compute_unit_limit,
                    cu_price: config.priority_fee,
                },
                sender: self,
            })
        })
    }

    /// 多签：`signers` 中第一个是 fee payer，其余为额外签名者。
    ///
    /// 与单签版唯一区别是签名者列表；config 语义完全一致。
    fn build_multisig_v1_tx<'a>(
        &'a self,
        ixs: &[Instruction],
        signers: &[&Keypair],
        tip: &Option<u64>,
        nonce: &HashParam,
        config: V1TxConfig,
        memo: Option<Vec<&str>>,
    ) -> Result<TxEnvelope<'a, Self>, Box<dyn std::error::Error + Send + Sync>>
    where
        Self: Sync + Send + Sized + Display + SendTx + BuildTx,
    {
        use solana_sdk::message::v1::Message as V1Message;
        use solana_sdk::message::VersionedMessage;
        log_time!("build multisig V1 transaction", {
            if signers.is_empty() {
                return Err("build_multisig_v1_tx: signers is empty".into());
            }
            let hash = *nonce.hash();
            let payer = signers[0].pubkey();
            let mut instructions = Vec::new();

            if let HashParam::NonceAccount { account, authority, .. } = nonce {
                instructions.push(advance_nonce_account(account, authority));
            }

            if self.uses_tip_transfer() {
                if tip != &Some(0) {
                    let tip_address = self.get_tip_address();
                    let mut tip_amt = tip.unwrap_or(self.get_min_tip_amount());
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

            let message =
                V1Message::try_compile_with_config(&payer, &instructions, hash, config.to_transaction_config())?;
            let transaction = VersionedTransaction::try_new(VersionedMessage::V1(message), signers)?;
            let sig = transaction.signatures[0];
            info!("  sig: {}", sig);
            Ok(TxEnvelope {
                tx: DetailedTx {
                    tx: transaction,
                    platform: self.platform(),
                    tip: *tip,
                    cu_limit: config.compute_unit_limit,
                    cu_price: config.priority_fee,
                },
                sender: self,
            })
        })
    }
}

/// V1 交易级配置（`BuildV1Tx` 的入参）。
///
/// 对应消息里的 `v1::TransactionConfig`，但用 `Option` 表达"未设置"，
/// 与 V0 的 `cu: (Option<u32>, Option<u64>)` 习惯保持一致。
///
/// # 默认值语义（与 V0 不同，务必注意）
///
/// | 字段 | 未设置时的链上行为 |
/// |---|---|
/// | `compute_unit_limit` | **取 0**（不是"无限制"） |
/// | `priority_fee` | **取 0**（不额外付费） |
/// | `loaded_accounts_data_size_limit` | **取 0** |
/// | `heap_size` | 32 KB |
#[derive(Debug, Clone, Copy, Default)]
pub struct V1TxConfig {
    /// 优先费，单位 **lamports**（不是 micro-lamports 单价）。
    pub priority_fee: Option<u64>,
    /// 最大 compute unit。
    pub compute_unit_limit: Option<u32>,
    /// 最大可加载账户数据字节数。
    pub loaded_accounts_data_size_limit: Option<u32>,
    /// 堆大小（字节），必须是 1024 的倍数。
    pub heap_size: Option<u32>,
}

impl V1TxConfig {
    /// 转成消息用的 `TransactionConfig`。
    pub fn to_transaction_config(self) -> solana_sdk::message::v1::TransactionConfig {
        solana_sdk::message::v1::TransactionConfig {
            priority_fee: self.priority_fee,
            compute_unit_limit: self.compute_unit_limit,
            loaded_accounts_data_size_limit: self.loaded_accounts_data_size_limit,
            heap_size: self.heap_size,
        }
    }

    /// 从 `(cu_limit, cu_price)` 构造 —— 便于 V0 调用点平滑迁移。
    ///
    /// ⚠️ 单位不同：V0 的 `cu_price` 是 **micro-lamports 单价**，
    /// V1 的 `priority_fee` 是 **lamports 总额**。这里直接赋值，**不做换算**，
    /// 调用方需自行确认语义。
    pub fn from_cu(cu_limit: Option<u32>, cu_price: Option<u64>) -> Self {
        Self {
            compute_unit_limit: cu_limit,
            priority_fee: cu_price,
            ..Default::default()
        }
    }
}

// 各平台 BuildV1Tx 实现（与 BuildV0Tx 一一对应）
impl BuildV1Tx for astralane::Astralane {}
impl BuildV1Tx for astralane_quic::client::AstralaneQuic {}
impl BuildV1Tx for blockrazor::Blockrazor {}
impl BuildV1Tx for helius_max::HeliusMax {}
impl BuildV1Tx for helius_swqos::HeliusSwqos {}
impl BuildV1Tx for harmonic::HarmonicBlockEngine {}
impl BuildV1Tx for jito::Jito {}
impl BuildV1Tx for nodeone::NodeOne {}
impl BuildV1Tx for temporal::Temporal {}
impl BuildV1Tx for zeroslot::ZeroSlot {}
impl BuildV1Tx for flash_block::FlashBlock {}
impl BuildV1Tx for nextblock::NextBlock {}
impl BuildV1Tx for stellium::Stellium {}
impl BuildV1Tx for ever_stake::EverStake {}
impl BuildV1Tx for ever_stake_quic::EverStakeQuic {}

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

/// 验证 V1 交易组装：config 正确落到消息里，且不再有 ComputeBudget 指令。
#[test]
fn test_build_v1_tx_uses_config_not_compute_budget_ix() {
    use crate::platform_clients::ever_stake::EverStake;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::Instruction;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Keypair;

    let sender = EverStake::new();
    let signer = std::sync::Arc::new(Keypair::new());
    let nonce = HashParam::Blockhash(Hash::new_from_array([9u8; 32]));

    // 一条用户指令（System transfer）
    let ix = solana_system_interface::instruction::transfer(
        &signer.pubkey(),
        &Pubkey::new_unique(),
        1,
    );

    let config = V1TxConfig {
        compute_unit_limit: Some(200_000),
        priority_fee: Some(5_000),
        loaded_accounts_data_size_limit: Some(65_536),
        heap_size: Some(32_768),
    };

    let envelope = sender
        .build_v1_tx(&[ix], &signer, &None, &nonce, config, None)
        .expect("V1 构建应成功");

    // 1. 必须是 V1 消息
    match &envelope.tx.tx.message {
        solana_sdk::message::VersionedMessage::V1(m) => {
            // 2. config 必须原样落到消息里
            assert_eq!(m.config.compute_unit_limit, Some(200_000));
            assert_eq!(m.config.priority_fee, Some(5_000));
            assert_eq!(m.config.loaded_accounts_data_size_limit, Some(65_536));
            assert_eq!(m.config.heap_size, Some(32_768));

            // 3. 指令里**不应**出现 ComputeBudgetProgram
            //    （V1 的 CU 在 config 里，不是指令）
            let has_cb_ix = m.instructions.iter().any(|ci| {
                m.account_keys
                    .get(ci.program_id_index as usize)
                    .map(|k| *k == const_accounts::COMPUTE_BUDGET_PROGRAM)
                    .unwrap_or(false)
            });
            assert!(!has_cb_ix, "V1 交易不应包含 ComputeBudgetProgram 指令");
        }
        other => panic!("期望 V1 消息，实际是 {other:?}"),
    }

    // 4. DetailedTx 里的 cu 值应来自 config
    assert_eq!(envelope.tx.cu_limit, Some(200_000));
    assert_eq!(envelope.tx.cu_price, Some(5_000));

    // 5. 签名存在
    assert_eq!(envelope.tx.tx.signatures.len(), 1);
}

/// 验证 V1 能装下超过 1232 字节的交易（V0 会失败）。
#[test]
fn test_v1_accepts_larger_than_v0_limit() {
    use crate::platform_clients::ever_stake::EverStake;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::Instruction;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Keypair;
    use solana_sdk::instruction::AccountMeta;

    let sender = EverStake::new();
    let signer = std::sync::Arc::new(Keypair::new());
    let nonce = HashParam::Blockhash(Hash::new_from_array([3u8; 32]));

    // 构造一条 data 很大的指令（> 1232 字节）
    let big_ix = Instruction {
        program_id: Pubkey::new_unique(),
        accounts: vec![AccountMeta::new(Pubkey::new_unique(), false)],
        data: vec![0xAB; 3000],
    };

    let envelope = sender
        .build_v1_tx(
            &[big_ix],
            &signer,
            &None,
            &nonce,
            V1TxConfig {
                compute_unit_limit: Some(1_000_000),
                ..Default::default()
            },
            None,
        )
        .expect("V1 应能容纳超过 1232 字节的交易");

    // 序列化后应超过旧的 1232 上限
    let bytes = bincode::serialize(&envelope.tx.tx).unwrap();
    assert!(
        bytes.len() > 1232,
        "该交易应超过 V0 上限，实际 {} 字节",
        bytes.len()
    );
    assert!(bytes.len() <= 4096, "不应超过 V1 上限，实际 {} 字节", bytes.len());
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
