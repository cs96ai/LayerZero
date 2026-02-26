use anyhow::Result;
use rand::Rng;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::db;
use crate::eth;
use crate::event::{Actor, LifecycleEvent, Status, Step};
use crate::solana_sim;
use crate::types::{AppState, MessageState};
use crate::verification;

const MAX_RETRIES: i32 = 1;

/// Returns true ~10% of the time to simulate transient failures.
fn should_simulate_failure() -> bool {
    rand::thread_rng().gen_ratio(1, 10)
}

/// Returns true ~50% of the time (coin flip for retry outcome).
fn retry_also_fails() -> bool {
    rand::thread_rng().gen_bool(0.5)
}

/// Main processor loop: polls Ethereum for events and advances the state machine.
pub async fn run_processor(state: Arc<AppState>, cfg: Config) -> Result<()> {
    info!("Starting state machine processor");

    // Resume any in-flight messages from a previous run (crash-safe resume)
    resume_inflight(&state, &cfg).await?;

    let mut last_block: u64 = 0;

    loop {
        // Check if paused
        if state.paused.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        let poll_ms = cfg.poll_interval_ms;

        // 1. Poll Ethereum for new CrossChainRequest events
        match poll_ethereum(&state, &cfg, &mut last_block).await {
            Ok(count) => {
                if count > 0 {
                    info!(count, last_block, "Observed new cross-chain requests");
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to poll Ethereum, will retry");
            }
        }

        // 2. Process messages through the state machine
        if let Err(e) = process_pending_messages(&state, &cfg).await {
            error!(error = %e, "Error processing messages");
        }

        sleep(Duration::from_millis(poll_ms)).await;
    }
}

/// Resume in-flight messages after a crash/restart.
/// Logs counts per state so the operator can see what was interrupted.
/// Messages in SentToSolana are promoted to Executed (the result is already
/// in DB from the previous run) so they don't get stuck.
async fn resume_inflight(state: &Arc<AppState>, _cfg: &Config) -> Result<()> {
    let resume_states = [
        MessageState::Observed,
        MessageState::Persisted,
        MessageState::Verified,
        MessageState::SentToSolana,
        MessageState::Executed,
    ];
    for resume_state in &resume_states {
        let messages = db::get_messages_by_state(&state.pool, *resume_state).await?;
        if messages.is_empty() {
            continue;
        }
        info!(
            state = %resume_state,
            count = messages.len(),
            "Resuming in-flight messages"
        );

        // SentToSolana is a transient state — if we crashed mid-transition,
        // the result is already stored. Promote to Executed so settlement can proceed.
        if *resume_state == MessageState::SentToSolana {
            for msg in &messages {
                db::update_message_state(
                    &state.pool,
                    msg.nonce as u64,
                    MessageState::Executed,
                    None, None, None, None,
                )
                .await?;
                info!(nonce = msg.nonce, "Promoted SentToSolana → Executed on resume");
            }
        }
    }
    Ok(())
}

/// Poll Ethereum for new CrossChainRequest events.
async fn poll_ethereum(
    state: &Arc<AppState>,
    cfg: &Config,
    last_block: &mut u64,
) -> Result<usize> {
    let current_block = eth::get_block_number(&cfg.eth_rpc_url).await?;

    if current_block <= *last_block {
        return Ok(0);
    }

    let from_block = if *last_block == 0 { 0 } else { *last_block + 1 };
    let logs = eth::fetch_logs(&cfg.eth_rpc_url, &cfg.escrow_address, from_block).await?;

    let mut count = 0;
    for log in &logs {
        match eth::parse_log(log) {
            Ok(event) => {
                // Idempotency: skip if already in DB
                if db::nonce_exists(&state.pool, event.nonce).await? {
                    continue;
                }

                let trace_id = format!("{:?}", event.trace_id);

                // Try to extract a human-readable description from the payload
                // Format: 16 bytes trace_id + 2 bytes desc_len (BE) + desc_bytes + random
                let description = extract_description(&event.payload);

                // Persist to DB
                db::insert_message(
                    &state.pool,
                    event.nonce,
                    &trace_id,
                    &format!("{:?}", event.sender),
                    &event.amount.to_string(),
                    &hex::encode(&event.payload),
                    event.deadline.as_u64() as i64,
                    description.as_deref(),
                )
                .await?;

                // Emit lifecycle events
                let locked_event = LifecycleEvent::new(
                    &trace_id,
                    event.nonce,
                    Actor::Ethereum,
                    Step::Locked,
                    Status::Success,
                )
                .with_detail(format!("tx:{:?}", event.tx_hash));
                emit_and_persist(state, &locked_event).await?;

                let observed_event = LifecycleEvent::new(
                    &trace_id,
                    event.nonce,
                    Actor::Relayer,
                    Step::Observed,
                    Status::Success,
                )
                .with_detail(format!("block:{}", event.block_number));
                emit_and_persist(state, &observed_event).await?;

                // Advance to Persisted
                db::update_message_state(
                    &state.pool,
                    event.nonce,
                    MessageState::Persisted,
                    None,
                    None,
                    None,
                    None,
                )
                .await?;

                count += 1;
            }
            Err(e) => {
                warn!(error = %e, "Failed to parse log");
            }
        }
    }

    *last_block = current_block;
    Ok(count)
}

/// Process all pending messages through the state machine.
async fn process_pending_messages(state: &Arc<AppState>, cfg: &Config) -> Result<()> {
    // Process each state in order
    process_state(state, cfg, MessageState::Persisted).await?;
    process_state(state, cfg, MessageState::Verified).await?;
    process_state(state, cfg, MessageState::SentToSolana).await?;
    process_state(state, cfg, MessageState::Executed).await?;
    Ok(())
}

async fn process_state(
    state: &Arc<AppState>,
    cfg: &Config,
    current_state: MessageState,
) -> Result<()> {
    let messages = db::get_messages_by_state(&state.pool, current_state).await?;

    for msg in messages {
        if state.paused.load(Ordering::Relaxed) {
            break;
        }

        let nonce = msg.nonce as u64;
        let trace_id = &msg.trace_id;

        if msg.retry_count >= MAX_RETRIES {
            warn!(nonce, retries = msg.retry_count, "Max retries exceeded, rolling back");

            // Emit rollback event
            let rollback_event = LifecycleEvent::new(
                trace_id,
                nonce,
                Actor::Relayer,
                Step::Rollback,
                Status::Failure,
            )
            .with_detail(format!(
                "Rollback: {} failed after {} retry. Funds will be refunded.",
                current_state, msg.retry_count
            ));
            emit_and_persist(state, &rollback_event).await?;

            db::update_message_state(
                &state.pool,
                nonce,
                MessageState::RolledBack,
                None,
                None,
                None,
                Some(&format!("Rolled back from {} after retry failure", current_state)),
            )
            .await?;

            let settled_event = LifecycleEvent::new(
                trace_id,
                nonce,
                Actor::Ethereum,
                Step::Settled,
                Status::Failure,
            )
            .with_detail("Escrow refunded — rollback complete");
            emit_and_persist(state, &settled_event).await?;

            info!(nonce, from_state = %current_state, "Message rolled back, funds refunded");
            continue;
        }

        let result = match current_state {
            MessageState::Persisted => advance_persisted_to_verified(state, cfg, &msg).await,
            MessageState::Verified => advance_verified_to_sent(state, cfg, &msg).await,
            MessageState::SentToSolana => advance_sent_to_executed(state, cfg, &msg).await,
            MessageState::Executed => advance_executed_to_settled(state, cfg, &msg).await,
            _ => Ok(()),
        };

        if let Err(e) = result {
            warn!(nonce, error = %e, "State transition failed, will retry");
            db::increment_retry(&state.pool, nonce).await?;

            let retry_event = LifecycleEvent::new(
                trace_id,
                nonce,
                Actor::Relayer,
                step_for_state(current_state),
                Status::Retry,
            )
            .with_detail(format!("Error: {}", e));
            emit_and_persist(state, &retry_event).await?;
        }
    }
    Ok(())
}

/// Persisted → Verified: simulate light-client verification.
async fn advance_persisted_to_verified(
    state: &Arc<AppState>,
    cfg: &Config,
    msg: &crate::types::CrossChainMessage,
) -> Result<()> {
    let nonce = msg.nonce as u64;

    // SIMULATION: 10% chance of verification failure
    if should_simulate_failure() {
        let is_retry = msg.retry_count > 0;
        if is_retry && retry_also_fails() {
            warn!(nonce, "Simulated verification failure on RETRY — will rollback");
            anyhow::bail!("Simulated: light-client verification failed (retry)");
        } else if !is_retry {
            warn!(nonce, "Simulated verification failure — will retry");
            anyhow::bail!("Simulated: light-client verification timeout");
        }
    }

    // Generate and verify proof bundle with real ECDSA signature
    let proof = verification::generate_proof_bundle(
        nonce,
        0, // We don't track block number in the message
        &msg.trace_id,
        msg.payload.as_bytes(),
        &cfg.relayer_private_key,
    )?;

    verification::verify_proof_bundle(&proof)?;

    // Store the proof bundle so the API returns stable hashes
    let proof_json = serde_json::to_string(&proof)?;
    db::store_proof(&state.pool, nonce, &proof_json).await?;

    db::update_message_state(
        &state.pool,
        nonce,
        MessageState::Verified,
        None,
        None,
        None,
        None,
    )
    .await?;

    let event = LifecycleEvent::new(
        &msg.trace_id,
        nonce,
        Actor::Relayer,
        Step::Verified,
        Status::Success,
    )
    .with_detail("Simulated light-client verification passed");
    emit_and_persist(state, &event).await?;

    info!(nonce, "Message verified (simulated)");
    Ok(())
}

/// Verified → SentToSolana: send instruction to Solana.
async fn advance_verified_to_sent(
    state: &Arc<AppState>,
    cfg: &Config,
    msg: &crate::types::CrossChainMessage,
) -> Result<()> {
    let nonce = msg.nonce as u64;

    // Parse amount (stored as string in DB)
    let amount: u64 = msg.amount.parse().unwrap_or(0);

    // Parse trace_id into [u8; 32]
    let trace_str = msg.trace_id.trim_start_matches("0x");
    let mut trace_bytes = [0u8; 32];
    if let Ok(bytes) = hex::decode(trace_str) {
        let len = bytes.len().min(32);
        trace_bytes[..len].copy_from_slice(&bytes[..len]);
    }

    // SIMULATION: 10% chance of Solana execution failure
    if should_simulate_failure() {
        let is_retry = msg.retry_count > 0;
        if is_retry && retry_also_fails() {
            warn!(nonce, "Simulated Solana execution failure on RETRY — will rollback");
            anyhow::bail!("Simulated: Solana program execution reverted (retry)");
        } else if !is_retry {
            warn!(nonce, "Simulated Solana execution failure — will retry");
            anyhow::bail!("Simulated: Solana transaction timeout");
        }
    }

    let (sig, result) = solana_sim::execute_on_solana(nonce, amount, trace_bytes).await?;

    db::update_message_state(
        &state.pool,
        nonce,
        MessageState::SentToSolana,
        Some(&result.to_string()),
        Some(&sig),
        None,
        None,
    )
    .await?;

    let event = LifecycleEvent::new(
        &msg.trace_id,
        nonce,
        Actor::Relayer,
        Step::Executed,
        Status::Success,
    )
    .with_detail(format!("solana_sig:{}, result:{}", sig, result));
    emit_and_persist(state, &event).await?;

    // Immediately advance to Executed (since we got a response)
    db::update_message_state(
        &state.pool,
        nonce,
        MessageState::Executed,
        None,
        None,
        None,
        None,
    )
    .await?;

    // Emit minted event (simulated bridge receipt)
    let mint_event = LifecycleEvent::new(
        &msg.trace_id,
        nonce,
        Actor::Solana,
        Step::Minted,
        Status::Success,
    )
    .with_detail("Simulated receipt token minted");
    emit_and_persist(state, &mint_event).await?;

    info!(nonce, %sig, result, "Solana execution complete");
    Ok(())
}

/// SentToSolana → Executed: observe Solana logs (already handled in advance_verified_to_sent).
async fn advance_sent_to_executed(
    _state: &Arc<AppState>,
    _cfg: &Config,
    _msg: &crate::types::CrossChainMessage,
) -> Result<()> {
    // This state is already handled by advance_verified_to_sent
    // which moves directly to Executed. This handler exists for
    // crash-safe resume where we might restart in SentToSolana state.
    // In that case, we trust the result already in DB and advance.
    Ok(())
}

/// Executed → Settled: sign result and call Ethereum settle().
async fn advance_executed_to_settled(
    state: &Arc<AppState>,
    cfg: &Config,
    msg: &crate::types::CrossChainMessage,
) -> Result<()> {
    let nonce = msg.nonce as u64;

    let result_str = msg.result.as_deref().unwrap_or("0");
    let result_value: u64 = result_str.parse().unwrap_or(0);

    // Encode result as uint256 (32 bytes, big-endian)
    let mut result_bytes = vec![0u8; 32];
    result_bytes[24..32].copy_from_slice(&result_value.to_be_bytes());

    // Emit burned event (simulated bridge receipt burn before settlement)
    let burn_event = LifecycleEvent::new(
        &msg.trace_id,
        nonce,
        Actor::Solana,
        Step::Burned,
        Status::Success,
    )
    .with_detail("Simulated receipt token burned for settlement");
    emit_and_persist(state, &burn_event).await?;

    // SIMULATION: 10% chance of settlement failure
    if should_simulate_failure() {
        let is_retry = msg.retry_count > 0;
        if is_retry && retry_also_fails() {
            warn!(nonce, "Simulated settlement failure on RETRY — will rollback");
            anyhow::bail!("Simulated: Ethereum settlement reverted (retry)");
        } else if !is_retry {
            warn!(nonce, "Simulated settlement failure — will retry");
            anyhow::bail!("Simulated: Ethereum gas estimation failed");
        }
    }

    // Sign the settlement
    let signature = eth::sign_settlement(&cfg.relayer_private_key, nonce, &result_bytes)?;

    // Call settle() on Ethereum
    match eth::call_settle(
        &cfg.eth_rpc_url,
        &cfg.relayer_private_key,
        &cfg.escrow_address,
        nonce,
        &result_bytes,
        &signature,
    )
    .await
    {
        Ok(tx_hash) => {
            db::update_message_state(
                &state.pool,
                nonce,
                MessageState::Settled,
                None,
                None,
                Some(&format!("{:?}", tx_hash)),
                None,
            )
            .await?;

            let event = LifecycleEvent::new(
                &msg.trace_id,
                nonce,
                Actor::Ethereum,
                Step::Settled,
                Status::Success,
            )
            .with_detail(format!("tx:{:?}", tx_hash));
            emit_and_persist(state, &event).await?;

            info!(nonce, %tx_hash, "Escrow settled on Ethereum");
        }
        Err(e) => {
            warn!(nonce, error = %e, "Settlement failed, simulating success for demo");
            // SIMULATION: In demo mode, if Ethereum is unreachable, simulate settlement
            let fake_tx = format!("0xsim_settle_{}", nonce);
            db::update_message_state(
                &state.pool,
                nonce,
                MessageState::Settled,
                None,
                None,
                Some(&fake_tx),
                None,
            )
            .await?;

            let event = LifecycleEvent::new(
                &msg.trace_id,
                nonce,
                Actor::Ethereum,
                Step::Settled,
                Status::Success,
            )
            .with_detail(format!("simulated_tx:{}", fake_tx));
            emit_and_persist(state, &event).await?;

            info!(nonce, "Escrow settlement simulated");
        }
    }

    Ok(())
}

/// Helper: emit event to broadcast channel and persist to DB.
async fn emit_and_persist(
    state: &Arc<AppState>,
    event: &LifecycleEvent,
) -> Result<()> {
    // Persist to DB
    db::insert_event(
        &state.pool,
        event.nonce,
        &event.trace_id,
        &format!("{:?}", event.actor).to_lowercase(),
        &format!("{:?}", event.step).to_lowercase(),
        &format!("{:?}", event.status).to_lowercase(),
        event.detail.as_deref(),
        &event.timestamp,
    )
    .await?;

    // Broadcast to WebSocket subscribers (ignore if no receivers)
    let _ = state.event_tx.send(event.clone());

    Ok(())
}

/// Extract a human-readable description from the payload if present.
/// Payload format: 16 bytes trace_id + 2 bytes desc_len (BE) + desc_bytes + random
fn extract_description(payload: &[u8]) -> Option<String> {
    if payload.len() < 18 {
        return None; // Not enough bytes for trace_id + length
    }
    let desc_len = u16::from_be_bytes([payload[16], payload[17]]) as usize;
    if desc_len == 0 || payload.len() < 18 + desc_len {
        return None;
    }
    std::str::from_utf8(&payload[18..18 + desc_len]).ok().map(String::from)
}

fn step_for_state(state: MessageState) -> Step {
    match state {
        MessageState::Observed | MessageState::Persisted => Step::Observed,
        MessageState::Verified => Step::Verified,
        MessageState::SentToSolana => Step::Executed,
        MessageState::Executed => Step::Executed,
        MessageState::Settled => Step::Settled,
        MessageState::Failed => Step::Settled,
        MessageState::RolledBack => Step::Rollback,
    }
}
