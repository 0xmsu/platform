#![allow(dead_code, unused_variables, unused_imports)]
//! Bittensor Integration for Mini-Chain
//!
//! Connects the Mini-Chain P2P layer to Bittensor blockchain
//! for submitting weights and reading metagraph state.
//!
//! Features:
//! - Validators synced from Bittensor metagraph
//! - Block subscription for epoch synchronization
//! - Weight submission via mechanism-based batching
//! - Concurrent weight collection from challenge endpoints
//!
//! The `BlockSync` module subscribes to finalized Bittensor blocks
//! to synchronize platform epochs with on-chain state.

mod block_sync;
mod challenge_weight_collector;
mod client;
mod config;
mod validator_sync;
mod weights;
mod weights_v2;

#[cfg(test)]
mod tests;

pub use block_sync::*;
pub use challenge_weight_collector::*;
pub use client::*;
pub use config::*;
pub use validator_sync::*;
pub use weights::*;
pub use weights_v2::{normalize_weights_f32, normalize_weights_f64, WeightSubmitterV2};

// Re-export bittensor-rs types for convenience
pub use bittensor_rs::BittensorClient;

// Re-export high-level Subtensor API
pub use bittensor_rs::{
    PendingCommit, Salt, Subtensor, SubtensorBuilder, SubtensorState, WeightResponse,
    WeightResponseData,
};
