//! Astralane QUIC Client Module
//!
//! This module provides a high-performance QUIC-based client for interacting with
//! Astralane's QUIC endpoints. It offers lower latency and better concurrency
//! compared to the HTTP-based client.

pub mod client;
pub mod config;

pub use client::AstralaneQuic;
pub use config::{AstralaneQuicConfig, get_quic_endpoint, limits};
