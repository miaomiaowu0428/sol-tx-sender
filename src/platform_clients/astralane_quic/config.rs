//! Astralane QUIC Client Configuration
//!
//! This file contains the configuration for Astralane QUIC endpoints and client settings.
//! Based on official Astralane QUIC documentation.

use crate::platform_clients::Region;

/// QUIC endpoints by region based on official Astralane documentation
/// Recommended endpoints are marked with comments
pub const ASTRALANE_QUIC_ENDPOINTS: &[&str] = &[
    // Frankfurt (Recommended) - IP: 185.191.117.97:7000
    "185.191.117.97:7000",
    // Frankfurt (Alternative) - IP: 45.139.132.160:7000
    "45.139.132.160:7000",
    // San Francisco - IP: 74.118.142.151:7000
    "74.118.142.151:7000",
    // Tokyo - IP: 189.1.164.31:7000
    "189.1.164.31:7000",
    // New York - IP: 64.130.45.19:7000
    "64.130.45.19:7000",
    // Amsterdam (Recommended) - IP: 64.130.43.43:7000
    "64.130.43.43:7000",
    // Amsterdam (Alternative) - IP: 84.32.186.73:7000
    "84.32.186.73:7000",
    // Limburg - IP: 162.19.222.232:7000
    "162.19.222.232:7000",
    // Singapore - IP: 67.209.54.176:7000
    "67.209.54.176:7000",
    // Lithuania - IP: 84.32.97.47:7000
    "84.32.97.47:7000",
];

/// QUIC server limits based on official documentation
pub mod limits {
    /// Max connections per API key
    pub const MAX_CONNECTIONS_PER_API_KEY: u32 = 10;

    /// Max concurrent streams per connection
    pub const MAX_CONCURRENT_STREAMS: u32 = 64;

    /// Stream read timeout in milliseconds
    pub const STREAM_READ_TIMEOUT_MS: u64 = 750;

    /// Max transaction size in bytes (standard Solana limit)
    pub const MAX_TRANSACTION_SIZE: usize = 1232;

    /// Idle timeout in seconds (connection closes if no activity)
    pub const IDLE_TIMEOUT_SECS: u64 = 30;

    /// Keep-alive interval in seconds (client sends keep-alive pings)
    pub const KEEP_ALIVE_INTERVAL_SECS: u64 = 25;
}

/// QUIC error codes from server
pub mod error_codes {
    /// Normal closure (client or server initiated)
    pub const OK: u32 = 0;

    /// API key not recognized
    pub const UNKNOWN_API_KEY: u32 = 1;

    /// Too many connections for this API key
    pub const CONNECTION_LIMIT: u32 = 2;
}

/// Get the appropriate QUIC endpoint based on region
pub fn get_quic_endpoint(region: &Region) -> &'static str {
    match region {
        Region::Frankfurt => ASTRALANE_QUIC_ENDPOINTS[0], // Recommended Frankfurt
        Region::LosAngeles => ASTRALANE_QUIC_ENDPOINTS[2], // San Francisco
        Region::Tokyo => ASTRALANE_QUIC_ENDPOINTS[3],     // Tokyo
        Region::NewYork => ASTRALANE_QUIC_ENDPOINTS[4],   // New York
        Region::Amsterdam => ASTRALANE_QUIC_ENDPOINTS[5], // Recommended Amsterdam
        Region::Singapore => ASTRALANE_QUIC_ENDPOINTS[8], // Singapore
        Region::Limburg => ASTRALANE_QUIC_ENDPOINTS[7],   // Limburg
        Region::Lithuania => ASTRALANE_QUIC_ENDPOINTS[9], // Lithuania
        _ => ASTRALANE_QUIC_ENDPOINTS[0],                 // Default to Frankfurt
    }
}

/// Get all endpoints for a specific region (for failover)
pub fn get_region_endpoints(region: &Region) -> Vec<&'static str> {
    match region {
        Region::Frankfurt => vec![
            ASTRALANE_QUIC_ENDPOINTS[0], // Recommended
            ASTRALANE_QUIC_ENDPOINTS[1], // Alternative
        ],
        Region::Amsterdam => vec![
            ASTRALANE_QUIC_ENDPOINTS[5], // Recommended
            ASTRALANE_QUIC_ENDPOINTS[6], // Alternative
        ],
        Region::LosAngeles => vec![ASTRALANE_QUIC_ENDPOINTS[2]],
        Region::Tokyo => vec![ASTRALANE_QUIC_ENDPOINTS[3]],
        Region::NewYork => vec![ASTRALANE_QUIC_ENDPOINTS[4]],
        Region::Singapore => vec![ASTRALANE_QUIC_ENDPOINTS[8]],
        Region::Limburg => vec![ASTRALANE_QUIC_ENDPOINTS[7]],
        Region::Lithuania => vec![ASTRALANE_QUIC_ENDPOINTS[9]],
        _ => vec![ASTRALANE_QUIC_ENDPOINTS[0]],
    }
}

/// Validate transaction size
pub fn validate_transaction_size(size: usize) -> Result<(), String> {
    if size > limits::MAX_TRANSACTION_SIZE {
        Err(format!(
            "Transaction size {} exceeds maximum allowed size {}",
            size,
            limits::MAX_TRANSACTION_SIZE
        ))
    } else {
        Ok(())
    }
}

/// Simplified configuration structure for Astralane QUIC client
/// Contains only essential connection parameters
#[derive(Debug, Clone)]
pub struct AstralaneQuicConfig {
    /// Server endpoint (IP:PORT format)
    pub endpoint: String,
    /// API key for authentication
    pub api_key: String,
}

impl AstralaneQuicConfig {
    /// Create a new config with endpoint and API key
    pub fn new(endpoint: String, api_key: String) -> Self {
        Self { endpoint, api_key }
    }
}
