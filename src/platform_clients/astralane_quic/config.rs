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

/// Get the appropriate QUIC endpoint based on region
pub fn get_quic_endpoint(region: &Region) -> &'static str {
    match region {
        Region::Frankfurt => ASTRALANE_QUIC_ENDPOINTS[0],  // Recommended Frankfurt
        Region::LosAngeles => ASTRALANE_QUIC_ENDPOINTS[2], // San Francisco
        Region::Tokyo => ASTRALANE_QUIC_ENDPOINTS[3],      // Tokyo
        Region::NewYork => ASTRALANE_QUIC_ENDPOINTS[4],    // New York
        Region::Amsterdam => ASTRALANE_QUIC_ENDPOINTS[5],  // Recommended Amsterdam
        Region::Singapore => ASTRALANE_QUIC_ENDPOINTS[8],  // Singapore
        Region::Limburg => ASTRALANE_QUIC_ENDPOINTS[7],    // Limburg
        Region::Lithuania => ASTRALANE_QUIC_ENDPOINTS[9],  // Lithuania
        _ => ASTRALANE_QUIC_ENDPOINTS[0],                  // Default to Frankfurt
    }
}
