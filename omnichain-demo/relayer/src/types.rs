use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, AtomicI64};
use tokio::sync::broadcast;

use crate::event::LifecycleEvent;

/// Shared application state across all tasks and handlers.
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<LifecycleEvent>,
    pub paused: AtomicBool,
    /// Whether the built-in traffic generator is running
    pub simulation_running: AtomicBool,
    /// Unix timestamp (seconds) when the simulation should auto-stop (0 = no deadline)
    pub simulation_deadline: AtomicI64,
}

/// Relayer state machine states for a cross-chain message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
pub enum MessageState {
    Observed,
    Persisted,
    Verified,
    SentToSolana,
    Executed,
    Settled,
    Failed,
    RolledBack,
}

impl std::fmt::Display for MessageState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Observed => write!(f, "observed"),
            Self::Persisted => write!(f, "persisted"),
            Self::Verified => write!(f, "verified"),
            Self::SentToSolana => write!(f, "sent_to_solana"),
            Self::Executed => write!(f, "executed"),
            Self::Settled => write!(f, "settled"),
            Self::Failed => write!(f, "failed"),
            Self::RolledBack => write!(f, "rolled_back"),
        }
    }
}

impl MessageState {
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "observed" => Self::Observed,
            "persisted" => Self::Persisted,
            "verified" => Self::Verified,
            "sent_to_solana" => Self::SentToSolana,
            "executed" => Self::Executed,
            "settled" => Self::Settled,
            "failed" => Self::Failed,
            "rolled_back" => Self::RolledBack,
            _ => Self::Failed,
        }
    }
}

/// Database row for a cross-chain message.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrossChainMessage {
    pub id: i64,
    pub nonce: i64,
    pub trace_id: String,
    pub sender: String,
    pub amount: String,
    pub payload: String,
    pub deadline: i64,
    pub description: Option<String>,
    pub state: String,
    pub result: Option<String>,
    pub solana_signature: Option<String>,
    pub eth_settle_tx: Option<String>,
    pub proof_json: Option<String>,
    pub retry_count: i32,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Simulated proof bundle for light-client verification.
/// SIMULATION: These fields are structurally correct but contain fabricated data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBundle {
    pub block_header: String,
    pub event_root: String,
    pub inclusion_proof: Vec<String>,
    pub validator_signature: String,
    pub relayer_address: String,
    pub nonce: u64,
    pub verified: bool,
}

/// API response types
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionListResponse {
    pub transactions: Vec<CrossChainMessage>,
    pub total: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionDetailResponse {
    pub transaction: CrossChainMessage,
    pub events: Vec<LifecycleEvent>,
    pub proof: Option<ProofBundle>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricsResponse {
    pub total_transactions: i64,
    pub settled: i64,
    pub failed: i64,
    pub pending: i64,
    pub total_retries: i64,
}

#[derive(Debug, Deserialize)]
pub struct SimulationRequest {
    /// Duration in minutes (default 60 = 1 hour)
    #[serde(default = "default_duration_minutes")]
    pub duration_minutes: u64,
}

fn default_duration_minutes() -> u64 {
    60
}

#[derive(Debug, Serialize)]
pub struct SimulationStatus {
    pub running: bool,
    pub remaining_seconds: i64,
}
