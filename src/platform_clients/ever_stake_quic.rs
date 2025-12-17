use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, Endpoint, ServerConfig, TransportConfig};
use solana_sdk::{
    message::Message,
    signature::{Keypair, Signer, read_keypair_file},
    transaction::Transaction,
};
use solana_system_interface::instruction;
use solana_tls_utils::{SkipServerVerification, new_dummy_x509_certificate};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::constants::REGION;

const ALPN_SWQOS_TX_PROTOCOL: &[&[u8]] = &[b"solana-tpu"];

pub struct EverStakeQuic {
    _endpoint: Endpoint,
    connection: Connection,
}

//Establish a connection to Everstake SWQoS Quic Endpoint
impl EverStakeQuic {
    pub fn get_endpoint() -> String {
        match *REGION {
            _ => "64.130.57.62:11809".to_string(),
        }
    }

    pub async fn new(keypair: &Keypair) -> Result<Self> {
        let (cert, key) = new_dummy_x509_certificate(keypair);

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

        println!("Transaction {signature:?} has been sent");
        Ok(())
    }
}
