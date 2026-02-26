use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Lifecycle event conforming to the shared event model.
/// All components emit events in this structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub trace_id: String,
    pub nonce: u64,
    pub actor: Actor,
    pub step: Step,
    pub status: Status,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    Ethereum,
    Relayer,
    Solana,
    Dashboard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Locked,
    Observed,
    Verified,
    Executed,
    Minted,
    Burned,
    Rollback,
    Settled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Failure,
    Retry,
}

impl LifecycleEvent {
    pub fn new(trace_id: &str, nonce: u64, actor: Actor, step: Step, status: Status) -> Self {
        Self {
            trace_id: trace_id.to_string(),
            nonce,
            actor,
            step,
            status,
            timestamp: Utc::now().to_rfc3339(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}
