//! **Solana wire 格式验证**（不发送任何交易）。
//!
//! 根因：`VersionedTransaction.signatures` 在 wire 格式里用 `short_vec`
//! （数量 < 128 时长度前缀 1 字节），而 `bincode` 会写成 u64（8 字节）——
//! 产出的字节不是合法 wire transaction，平台直接拒绝
//! （FlashBlock: `1015 Transaction has bad format`）。
//!
//! 本测试断言 `serialize_transaction_wire`（wincode）产出**真正的 wire 字节**：
//! 第 0 字节 = short_vec 长度（1），而不是 bincode 的 8 字节长度前缀。
//!
//! 跑法：`cargo test -p sol-tx-send --test tx_wire_format -- --nocapture`

use sol_tx_send::platform_clients::serialize_transaction_wire;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0, v1, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use std::sync::Arc;

fn dummy_ix() -> Instruction {
    // ⚠️ 不能放 is_signer:true 的账户（否则 try_compile 报 NotEnoughSigners，
    //    我们只签 payer 一个人）。
    Instruction {
        program_id: Pubkey::new_unique(),
        accounts: vec![
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new(Pubkey::new_unique(), false),
        ],
        data: vec![1, 2, 3, 4],
    }
}

#[test]
fn wire_v0_starts_with_short_vec_sig_count() {
    let payer = Arc::new(Keypair::new());
    let m = v0::Message::try_compile(&payer.pubkey(), &[dummy_ix()], &[], Hash::new_unique()).unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(m), &[payer.as_ref()]).unwrap();

    let wire = serialize_transaction_wire(&tx).expect("wire serialize");
    let bincode_bytes = bincode::serialize(&tx).expect("bincode serialize");

    println!("V0 wire    len = {}  head = {:02x?}", wire.len(), &wire[..10]);
    println!("V0 bincode len = {}  head = {:02x?}", bincode_bytes.len(), &bincode_bytes[..10]);

    // short_vec：1 个签名 → 第 0 字节是 0x01
    assert_eq!(wire[0], 1, "wire 第 0 字节应是 short_vec 签名数量（1 字节）");

    // ⚠️ 重要事实：**V0 下 bincode 与 wire 字节相同**。
    //    solana-sdk 的 `bincode` feature 给 VersionedTransaction 写了走
    //    short_vec 的专门 impl（见 solana-transaction/src/versioned/mod.rs
    //    的 `versioned_transaction_wincode_bincode_roundtrip` 测试，它断言
    //    两者相等）—— 但那个 impl **只覆盖 Legacy / V0，不认 V1**。
    assert_eq!(wire, bincode_bytes, "V0 下 bincode 与 wire 应一致（SDK 有专门 impl）");
}

/// **核心修复的验证**：V1 下 bincode 与 wire **不同** —— 这正是本次 bug 的根源。
///
/// SDK 的等价性测试只覆盖 Legacy / V0（`strat_versioned_message` 里没有 V1），
/// 所以 V1 会 fallback 到 bincode 的默认派生 → 长度前缀写成 u64 → 字节非法。
#[test]
fn bincode_differs_from_wire_for_v1() {
    let payer = Arc::new(Keypair::new());
    let cfg = v1::TransactionConfig {
        compute_unit_limit: Some(50_000),
        ..Default::default()
    };
    let m = v1::Message::try_compile_with_config(&payer.pubkey(), &[dummy_ix()], Hash::new_unique(), cfg).unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(m), &[payer.as_ref()]).unwrap();

    let wire = serialize_transaction_wire(&tx).expect("wire serialize");
    let by_bincode = bincode::serialize(&tx).expect("bincode serialize");

    println!("V1 wire     len = {}  head = {:02x?}", wire.len(), &wire[..12]);
    println!("V1 bincode  len = {}  head = {:02x?}", by_bincode.len(), &by_bincode[..12]);

    assert_ne!(
        wire, by_bincode,
        "V1 下 bincode 与 wire 必须不同（这就是 bincode 发出去被拒的原因）"
    );
    assert_eq!(wire[0], 0x81, "wire 形态第 0 字节应是 V1 判别字节 0x81（SIMD-0385）");
    // bincode 版本第 0 字节是 enum tag 的一部分，不会是 0x81
    assert_ne!(by_bincode[0], 0x81, "bincode 形态不可能是合法 V1 wire");
    println!(
        "✅ V1：wire={} 字节 / bincode={} 字节（差 {}，bincode 不是合法 wire）",
        wire.len(),
        by_bincode.len(),
        by_bincode.len() as isize - wire.len() as isize
    );
}

#[test]
fn wire_v1_has_version_discriminator_0x81() {
    let payer = Arc::new(Keypair::new());
    let cfg = v1::TransactionConfig {
        compute_unit_limit: Some(50_000),
        ..Default::default()
    };
    let m = v1::Message::try_compile_with_config(&payer.pubkey(), &[dummy_ix()], Hash::new_unique(), cfg).unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(m), &[payer.as_ref()]).unwrap();

    let wire = serialize_transaction_wire(&tx).expect("wire serialize");

    println!("V1 wire len = {}  head = {:02x?}", wire.len(), &wire[..16]);

    // SIMD-0385：v1 的判别字节是 0x81，且位于**偏移 0**（v0 是 0x80 在第 1 字节之后）
    assert_eq!(wire[0], 0x81, "V1 wire 第 0 字节应是版本判别字节 0x81");
    println!("✅ V1：判别字节 0x81 在偏移 0，符合 SIMD-0385");
}

#[test]
fn wire_base64_roundtrips_back() {
    use solana_sdk::transaction::VersionedTransaction as VT;

    let payer = Arc::new(Keypair::new());
    let cfg = v1::TransactionConfig {
        compute_unit_limit: Some(50_000),
        ..Default::default()
    };
    let m = v1::Message::try_compile_with_config(&payer.pubkey(), &[dummy_ix()], Hash::new_unique(), cfg).unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(m), &[payer.as_ref()]).unwrap();

    let b64 = sol_tx_send::platform_clients::serialize_transaction_wire_base64(&tx).expect("b64");
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64).expect("base64 decode");

    // 解回来应能被 SDK 认出来（证明是合法 wire 格式）
    let decoded = bincode::deserialize::<VT>(&bytes);
    // 注意：这里用 bincode 反序列化**也可能失败**（同样的长度前缀问题），
    // 所以改用 wincode 反序列化验证。
    let via_wincode = wincode::deserialize::<VT>(&bytes);
    println!("bincode deserialize ok? {}", decoded.is_ok());
    println!("wincode deserialize ok? {}", via_wincode.is_ok());

    assert!(via_wincode.is_ok(), "wincode 应能把 wire 字节解回交易（自洽性）");
    println!("✅ base64 往返：{} 字节 → {} 字符", bytes.len(), b64.len());
}
