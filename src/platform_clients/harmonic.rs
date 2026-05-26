//! Harmonic Block Engine 客户端（gRPC 正式实现）
//!
//! ## 与 Jito 的关键差异（来自官方文档）
//!
//! 1. **Tips = CU price，无需 SOL 转账**
//!    Harmonic 的 tip 就是交易的 compute unit price（priority fee），
//!    不需要额外的 SOL 转账指令。`uses_tip_transfer()` 返回 `false`，
//!    `build_v0_tx` 调用时应显式传 `tip = Some(0)` 跳过 tip 指令。
//!
//! 2. **Role = SEARCHER = 3**（Jito 是 1）
//!    如果你想复用 Jito protos，使用 `SHREDSTREAM_SUBSCRIBER = 3`。
//!    本实现使用 Harmonic 自己的 proto（auth.proto 中 `SEARCHER = 3`）。
//!
//! 3. **所有区域并发发送**
//!    官方建议同时发往所有 endpoint 以获得最低延迟。

use crate::platform_clients::harmonic_proto::{
    auth::{
        auth_service_client::AuthServiceClient, GenerateAuthChallengeRequest,
        GenerateAuthTokensRequest, Role,
    },
    bundle::Bundle,
    packet::{Meta, Packet, PacketFlags},
    searcher::{searcher_service_client::SearcherServiceClient, SendBundleRequest},
    shared::Header,
};
use crate::platform_clients::{PlatformName, Region};

use anyhow::{Context, anyhow};
use base64::Engine;
use log::{error, info};
use prost_types::Timestamp;
use solana_sdk::{pubkey, pubkey::Pubkey, signature::Keypair, signer::Signer};
use solana_sdk::transaction::VersionedTransaction;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::Request;

// ── 端点 ─────────────────────────────────────────────────────────────────────

pub const HARMONIC_BE_ENDPOINTS: &[&str] = &[
    "https://fra.be.harmonic.gg",  // Frankfurt
    "https://lon.be.harmonic.gg",  // London
    "https://ams.be.harmonic.gg",  // Amsterdam
    "https://ewr.be.harmonic.gg",  // Newark
    "https://tyo.be.harmonic.gg",  // Tokyo
    "https://sgp.be.harmonic.gg",  // Singapore
];

// ── HarmonicBlockEngine ───────────────────────────────────────────────────────

/// Harmonic Block Engine 客户端。
///
/// **Tip 说明**：Harmonic 的竞价通过 CU price 完成，不需要额外的 SOL 转账指令。
/// 在 `build_v0_tx` 调用时应传 `tip = Some(0)` 以跳过 tip 转账指令，
/// 竞价效果由 `cu_price` 参数决定。
#[derive(Clone)]
pub struct HarmonicBlockEngine {
    /// 并发发送到的所有 endpoint（官方建议全区域同发）
    endpoints: Vec<String>,
    /// 已被 Harmonic whitelist 的 searcher keypair
    searcher: Arc<Keypair>,
}

impl fmt::Display for HarmonicBlockEngine {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HarmonicBlockEngine")
    }
}

impl HarmonicBlockEngine {
    /// Harmonic 不收 tip，竞价靠 CU price，此处返回 0。
    /// 外部调用时应传 `tip = Some(0)` 确保不生成 tip 转账指令。
    pub const MIN_TIP_AMOUNT_TX: u64 = 0;

    /// 默认使用全部 endpoint（官方推荐）。
    pub fn init_with(searcher: Arc<Keypair>) -> Self {
        Self {
            endpoints: HARMONIC_BE_ENDPOINTS.iter().map(|s| s.to_string()).collect(),
            searcher,
        }
    }

    /// 指定部分 endpoint（用于测试或按需选择区域）。
    pub fn init_with_endpoints(searcher: Arc<Keypair>, endpoints: Vec<String>) -> Self {
        Self { endpoints, searcher }
    }

    /// 将序列化后的交易字节封装成 Harmonic bundle 并发往所有 endpoint。
    /// 并发发送，任一成功即视为整体成功（其余结果仍等待并记录日志）。
    async fn send_bundle_bytes(&self, tx_bytes: Vec<u8>) -> Result<(), String> {
        let packet_size = tx_bytes.len() as u64;

        let mut handles = Vec::with_capacity(self.endpoints.len());
        for endpoint in &self.endpoints {
            let endpoint = endpoint.clone();
            let tx_bytes = tx_bytes.clone();
            let searcher = Arc::clone(&self.searcher);

            handles.push(tokio::spawn(async move {
                let result =
                    send_to_endpoint(&endpoint, &searcher, tx_bytes, packet_size).await;
                (endpoint, result)
            }));
        }

        let mut any_ok = false;
        for handle in handles {
            match handle.await {
                Ok((ep, Ok(uuid))) => {
                    info!("[HarmonicBlockEngine] {} → uuid={}", region_name(&ep), uuid);
                    any_ok = true;
                }
                Ok((ep, Err(e))) => {
                    error!("[HarmonicBlockEngine] {} failed: {:#}", region_name(&ep), e);
                }
                Err(e) => {
                    error!("[HarmonicBlockEngine] task join error: {}", e);
                }
            }
        }

        if any_ok {
            Ok(())
        } else {
            Err("all Harmonic endpoints failed".to_string())
        }
    }
}

// ── trait 实现 ────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl crate::platform_clients::SendTxEncoded for HarmonicBlockEngine {
    /// 接收 base64 编码的交易，反序列化后通过 gRPC bundle 发送。
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        let tx_bytes = base64::prelude::BASE64_STANDARD
            .decode(tx_base64)
            .map_err(|e| format!("base64 decode failed: {}", e))?;

        // 验证反序列化（确保 tx_bytes 是合法的 VersionedTransaction）
        let _ = bincode::deserialize::<VersionedTransaction>(&tx_bytes)
            .map_err(|e| format!("deserialize tx failed: {}", e))?;

        self.send_bundle_bytes(tx_bytes).await
    }
}

impl crate::platform_clients::BuildTx for HarmonicBlockEngine {
    fn get_tip_address(&self) -> Pubkey {
        // uses_tip_transfer()=false 保证此方法永远不会被调用
        panic!("HarmonicBlockEngine 不支持 SOL tip 转账，请检查 uses_tip_transfer() 实现")
    }
    fn get_min_tip_amount(&self) -> u64 {
        // uses_tip_transfer()=false 保证此方法永远不会被用于实际算 tip
        panic!("HarmonicBlockEngine 不支持 SOL tip，请检查 uses_tip_transfer() 实现")
    }
    fn platform(&self) -> PlatformName {
        PlatformName::Harmonic
    }
    fn tip_recvs(&self) -> Vec<Pubkey> {
        vec![]
    }
    /// Harmonic 竞价靠 CU price，不使用 SOL tip 转账指令。
    /// 不管外部传入什么 tip 均被忽略。
    fn uses_tip_transfer(&self) -> bool {
        false
    }
}

// ── 内部 gRPC 函数 ────────────────────────────────────────────────────────────

/// 向单个 endpoint 认证并发送 bundle。
async fn send_to_endpoint(
    endpoint: &str,
    searcher: &Keypair,
    tx_bytes: Vec<u8>,
    packet_size: u64,
) -> anyhow::Result<String> {
    let token = authenticate(endpoint, searcher).await?;
    let channel = connect(endpoint).await?;
    let mut client = SearcherServiceClient::new(channel);

    let mut req = Request::new(SendBundleRequest {
        bundle: Some(Bundle {
            header: Some(Header { ts: Some(now_ts()?) }),
            packets: vec![Packet {
                data: tx_bytes,
                meta: Some(Meta {
                    size: packet_size,
                    addr: String::new(),
                    port: 0,
                    flags: Some(PacketFlags {
                        discard: false,
                        forwarded: false,
                        repair: false,
                        simple_vote_tx: false,
                        tracer_packet: false,
                        from_staked_node: false,
                    }),
                    sender_stake: 0,
                }),
            }],
        }),
    });

    let bearer = MetadataValue::try_from(format!("Bearer {token}"))
        .context("build bearer metadata failed")?;
    req.metadata_mut().insert("authorization", bearer);

    let resp = client
        .send_bundle(req)
        .await
        .context("SendBundle gRPC failed")?
        .into_inner();

    Ok(resp.uuid)
}

/// challenge-response 认证，返回 access token。
async fn authenticate(endpoint: &str, searcher: &Keypair) -> anyhow::Result<String> {
    let channel = connect(endpoint).await?;
    let mut client = AuthServiceClient::new(channel);
    let pubkey = searcher.pubkey();

    // 1. 请求 challenge（Role::Searcher = 3，Harmonic 专用，与 Jito 的 1 不同）
    let challenge_resp = client
        .generate_auth_challenge(GenerateAuthChallengeRequest {
            role: Role::Searcher as i32,
            pubkey: pubkey.to_bytes().to_vec(),
        })
        .await
        .context("GenerateAuthChallenge failed")?
        .into_inner();

    // 2. 签名 "{pubkey}-{challenge}"
    let challenge = format!("{}-{}", pubkey, challenge_resp.challenge);
    let signed = searcher.sign_message(challenge.as_bytes());

    // 3. 换取 access token
    let tokens = client
        .generate_auth_tokens(GenerateAuthTokensRequest {
            challenge,
            client_pubkey: pubkey.to_bytes().to_vec(),
            signed_challenge: signed.as_ref().to_vec(),
        })
        .await
        .context("GenerateAuthTokens failed")?
        .into_inner();

    tokens
        .access_token
        .ok_or_else(|| anyhow!("auth response missing access_token"))
        .map(|t| t.value)
}

async fn connect(endpoint: &str) -> anyhow::Result<Channel> {
    let ep = Channel::from_shared(endpoint.to_string())
        .with_context(|| format!("invalid endpoint: {endpoint}"))?
        .tls_config(ClientTlsConfig::new().with_enabled_roots())?;
    ep.connect().await.context("connect gRPC endpoint failed")
}

fn now_ts() -> anyhow::Result<Timestamp> {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock error")?;
    Ok(Timestamp {
        seconds: d.as_secs() as i64,
        nanos: d.subsec_nanos() as i32,
    })
}

fn region_name(endpoint: &str) -> &str {
    endpoint
        .trim_start_matches("https://")
        .split('.')
        .next()
        .unwrap_or(endpoint)
}
