use anyhow::{Context, Result};
use base64::Engine;
use log::{debug, info, warn};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, Endpoint};
use rand::seq::IndexedRandom;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::{signature::Keypair, transaction::Transaction};
use solana_tls_utils::{SkipServerVerification, new_dummy_x509_certificate};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fmt};
use utils::log_time;

use crate::constants::REGION;
use crate::platform_clients::ever_stake::EVER_STAKE_TIP_ACCOUNTS;
use crate::platform_clients::{BuildTx, BuildV0Tx, PlatformName, Region, SendTxEncoded, TxSend};

const ALPN_SWQOS_TX_PROTOCOL: &[&[u8]] = &[b"solana-tpu"];

pub struct EverStakeQuic {
    _endpoint: Endpoint,
    connection: Connection,
}

//Establish a connection to Everstake SWQoS Quic Endpoint
impl EverStakeQuic {
    pub const MIN_TIP_AMOUNT_TX: u64 = 0_000_500_000; // 单笔交易最低 tip
    pub const DEFAULT_TPS: u64 = 1;
    pub fn get_endpoint() -> String {
        match *REGION {
            Region::Frankfurt => "64.130.57.62:11809".to_string(),
            Region::NewYork => "64.130.59.154:11809".to_string(),
            Region::Amsterdam => "74.118.140.197:11809".to_string(),
            Region::Tokyo => "208.91.107.171:11809".to_string(),
            _ => "64.130.57.62:11809".to_string(),
        }
    }

    pub async fn new() -> Result<Self> {
        let keypair_base58_string = std::env::var("EVER_STAKE_QUIC_KEYPAIR").unwrap_or_default();
        let keypair = Keypair::from_base58_string(&keypair_base58_string);
        let (cert, key) = new_dummy_x509_certificate(&keypair);

        let mut crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_client_auth_cert(vec![cert], key)
            .context("failed to configure client certificate")?;

        crypto.alpn_protocols = ALPN_SWQOS_TX_PROTOCOL.iter().map(|p| p.to_vec()).collect();

        let client_crypto = QuicClientConfig::try_from(crypto)
            .context("failed to convert rustls config into quinn crypto config")?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(client_crypto));
        let mut transport_config = quinn::TransportConfig::default();

        // 设置保活间隔。例如每 10 秒发送一个 PING 帧
        // 如果不设置，默认为 None（不发送保活包）
        transport_config.keep_alive_interval(Some(Duration::from_secs(10)));

        // (可选) 设置空闲超时时间，如果 30 秒内没有任何活动且没有保活包，则断开连接
        // transport_config.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));

        client_config.transport_config(Arc::new(transport_config)); // 将配置注入

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config.clone());

        let connection = endpoint
            .connect_with(
                client_config,
                Self::get_endpoint().parse()?,
                "everstake_swqos",
            )?
            .await?;

        Ok(Self {
            _endpoint: endpoint,
            connection,
        })
    }

    // Send a transaction via quic using a unidirectional stream
    pub async fn send_transaction(&self, transaction: &Transaction) -> Result<()> {
        let signature = transaction
            .signatures
            .first()
            .expect("Transaction must have at least one signature");
        let serialized_tx = bincode::serialize(transaction)?;

        let mut send_stream = self.connection.open_uni().await?;
        send_stream.write_all(&serialized_tx).await?;
        send_stream.finish()?;

        info!("Transaction {signature:?} has been sent");
        Ok(())
    }

    // 核心逻辑：只管发字节，不关心内容
    pub async fn send_raw_transaction(&self, raw_tx: &[u8]) -> Result<()> {
        // 如果你依然需要提取 signature 用于打印日志，可以只解析前 64 字节（Solana 签名在最前面）
        // 或者直接跳过解析，只打印长度

        let mut send_stream = self.connection.open_uni().await?;
        send_stream.write_all(raw_tx).await?;
        send_stream.finish()?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SendTxEncoded for EverStakeQuic {
    async fn send_tx_encoded(&self, tx_base64: &str) -> Result<(), String> {
        log_time!("ever stake quic send: ", {
            // 只需要 Base64 解码一次
            let bytes = base64::prelude::BASE64_STANDARD
                .decode(tx_base64)
                .map_err(|e| e.to_string())?;

            // 直接发送解码后的字节，无需转成 Transaction 结构体
            self.send_raw_transaction(&bytes)
                .await
                .map_err(|e| format!("Everstake Quic send error: {}", e))
        })
    }
}

impl crate::platform_clients::BuildTx for EverStakeQuic {
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

impl fmt::Display for EverStakeQuic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EverStakeQuic")
    }
}

// #[tokio::test]
// async fn test_everstake_quic() -> Result<()> {
//     rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider())
//         .unwrap();
//     let everstake_keypair = Keypair::from_base58_string(
//         "g6uc977Rk5ZF4jFP5J2PwKt3qRF59ncg2oZVFLq4erP7aUJJAUY7hfjrLWU7BtHLbGqncJHckjFrFeNkRFuQXHP",
//     );
//     let payer_key_pair =
//         <Keypair as solana_sdk::signer::EncodableKey>::read_from_file("./test2.json").unwrap();
//     let everstake_quic = EverStakeQuic::new(&everstake_keypair).await?;
//     println!("Connected to Everstake SWQoS Quic Endpoint");

//     {
//         // 构造交易然后用send_transaction发送就行了
//         let recent_blockhash = crate::constants::JSON_RPC_CLIENT
//             .get_latest_blockhash()
//             .await
//             .context("failed to get recent blockhash")?;
//         let hash = crate::platform_clients::HashParam::Blockhash(recent_blockhash);

//         let tx = everstake_quic
//             .build_v0_tx(
//                 &[],
//                 &Arc::new(payer_key_pair),
//                 &None,
//                 &hash,
//                 &(None, None),
//                 &[],
//                 None,
//             )
//             .unwrap();
//         tx.send()
//             .await
//             .map(|res| println!("Everstake Quic tx sent: {:?}", res))
//             .unwrap();
//     }
//     tokio::time::sleep(Duration::from_secs(5)).await;
//     println!("Test completed after waiting for 5 seconds.");

//     Ok(())
// }
