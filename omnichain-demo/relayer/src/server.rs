use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info};

use crate::db;
use crate::types::{
    AppState, MetricsResponse, SimulationRequest, SimulationStatus,
    TransactionDetailResponse, TransactionListResponse,
};

/// Run the HTTP + WebSocket server.
pub async fn run_server(state: Arc<AppState>, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        // Transaction endpoints
        .route("/transactions", get(list_transactions))
        .route("/transactions/:nonce", get(get_transaction))
        // Metrics
        .route("/metrics", get(get_metrics))
        // Control endpoints
        .route("/control/pause", post(pause))
        .route("/control/resume", post(resume))
        // Simulation control
        .route("/control/start-simulation", post(start_simulation))
        .route("/control/stop-simulation", post(stop_simulation))
        .route("/control/simulation-status", get(simulation_status))
        // Data management
        .route("/control/clear-data", post(clear_data))
        // AI analysis
        .route("/analyze/:nonce", post(analyze_transaction))
        // WebSocket endpoint for real-time event streaming
        .route("/ws", get(ws_handler))
        // Health check
        .route("/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(state)
        // Serve the dashboard static files as a fallback.
        // If /dashboard/index.html exists, serve it; otherwise no-op.
        .fallback_service(
            ServeDir::new("/dashboard")
                .not_found_service(ServeFile::new("/dashboard/index.html"))
        );

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(%addr, "HTTP + WebSocket server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

// ──────────────────────────────────────────────
// HTTP Handlers
// ──────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

async fn list_transactions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TransactionListResponse>, StatusCode> {
    let messages = db::get_all_messages(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = messages.len() as i64;
    Ok(Json(TransactionListResponse {
        transactions: messages,
        total,
    }))
}

async fn get_transaction(
    State(state): State<Arc<AppState>>,
    Path(nonce): Path<u64>,
) -> Result<Json<TransactionDetailResponse>, StatusCode> {
    let msg = db::get_message_by_nonce(&state.pool, nonce)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let events = db::get_events_by_nonce(&state.pool, nonce)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Load stored proof bundle from DB (stable hashes, no regeneration)
    let proof = msg.proof_json.as_ref().and_then(|json| {
        serde_json::from_str::<crate::types::ProofBundle>(json).ok()
    });

    Ok(Json(TransactionDetailResponse {
        transaction: msg,
        events,
        proof,
    }))
}

async fn get_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MetricsResponse>, StatusCode> {
    let (total, settled, failed, pending, retries) = db::get_metrics(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(MetricsResponse {
        total_transactions: total,
        settled,
        failed,
        pending,
        total_retries: retries,
    }))
}

async fn pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.paused.store(true, Ordering::Relaxed);
    info!("Relayer paused");
    Json(serde_json::json!({"paused": true}))
}

async fn resume(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.paused.store(false, Ordering::Relaxed);
    info!("Relayer resumed");
    Json(serde_json::json!({"paused": false}))
}

async fn start_simulation(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SimulationRequest>,
) -> impl IntoResponse {
    let deadline = chrono::Utc::now().timestamp() + (req.duration_minutes as i64 * 60);
    state.simulation_deadline.store(deadline, Ordering::Relaxed);
    state.simulation_running.store(true, Ordering::Relaxed);
    state.paused.store(false, Ordering::Relaxed);
    info!(duration_minutes = req.duration_minutes, "Simulation started");
    Json(serde_json::json!({
        "running": true,
        "duration_minutes": req.duration_minutes,
        "deadline_unix": deadline
    }))
}

async fn stop_simulation(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    state.simulation_running.store(false, Ordering::Relaxed);
    state.paused.store(true, Ordering::Relaxed);
    state.simulation_deadline.store(0, Ordering::Relaxed);
    info!("Simulation stopped");
    Json(serde_json::json!({"running": false}))
}

async fn simulation_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let running = state.simulation_running.load(Ordering::Relaxed);
    let deadline = state.simulation_deadline.load(Ordering::Relaxed);
    let remaining = if deadline > 0 {
        (deadline - chrono::Utc::now().timestamp()).max(0)
    } else {
        0
    };
    Json(SimulationStatus {
        running,
        remaining_seconds: remaining,
    })
}

async fn clear_data(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, StatusCode> {
    // Stop simulation first
    state.simulation_running.store(false, Ordering::Relaxed);
    state.paused.store(true, Ordering::Relaxed);

    db::clear_all_data(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!("All demo data cleared");
    Ok(Json(serde_json::json!({"cleared": true})))
}

// ──────────────────────────────────────────────
// AI Analysis
// ──────────────────────────────────────────────

const ANALYSIS_SYSTEM_PROMPT: &str = r#"You are a senior blockchain ops analyst writing an internal report for a company that sells cross-chain relayer ("cross-chain server") infrastructure. Our customers do cross-chain trading (buy on Chain A, sell on Chain B). We operate the relayer + monitoring + SLA, so we care about reliability, settlement correctness, retries, gas/fee issues, customer impact, theft prevention, and agents gaming the system.

CONTEXT / ASSUMPTIONS
- The data comes directly from our DB (source of truth).
- This is a simulator right now; IGNORE any "Simulated …" wording entirely.
- Your output will be rendered as MARKDOWN on a React page.
- Produce a report useful to: (1) customer support, (2) relayer ops/on-call, (3) product/SLA owners.
- Do NOT browse the web. Use only the data provided.

For EACH transaction:
1) Determine the FINAL STATUS from the evidence: settled_success, settled_failure_refunded, pending, inconsistent_state, or unknown.
2) Explain the lifecycle in plain English (what happened and why it matters).
3) Identify operational risks (stuck funds risk, double-mint risk, replay risk, relayer key risk, fee/gas volatility, partial execution).
4) Compute and display key timing metrics (end-to-end, observe latency, verify latency, execute latency, settle latency) from timestamps.
5) Provide recommended actions for relayer ops + product (concrete, checkable).
6) Provide UI-friendly "badges" and "alerts" that the React page can show.

OUTPUT: Return ONLY Markdown (no code fences). No JSON.

MARKDOWN STRUCTURE (strict):

# Omnichain Transaction Analysis

## Summary
- **Transaction:** <txId>
- **Label:** <humanLabel>
- **Amount:** <wei> wei (<eth estimate if possible>)
- **Final Status:** <one of the statuses above> (confidence: <0-100>%)
- **Customer Impact:** <1-2 sentences>

## Lifecycle (What happened)
Write 4-8 bullet points describing each stage in order, referencing chain + stage + status + timestamp.

## Consistency Check (Cross-chain correctness)
- **Ethereum lock tx:** <id or "missing">
- **Solana execution sig:** <id or "missing">
- **Solana mint:** <present/absent/unknown>
- **Solana burn:** <present/absent/unknown>
- **Ethereum settle/refund tx:** <id or "missing">
- **Verdict:** consistent | possible stuck-funds risk | possible double-mint risk | unknown
- **Why:** <short reasoning>

## Timing & Reliability Metrics
- **Time to observe:** <seconds or "n/a">
- **Time to verify:** <seconds or "n/a">
- **Time to execute:** <seconds or "n/a">
- **Time to settle/refund:** <seconds or "n/a">
- **End-to-end:** <seconds or "n/a">
- **Retries / attempts:** <x / y>

## Risks (Ops + Security)
Provide a table:

| Risk | Severity | Evidence | Why it matters |
|------|----------|----------|----------------|

Severity is Low/Medium/High.

## Recommended Actions

### Relayer Ops (today)
A checklist of 3-8 actions with "how to verify" notes.

### Product / Engineering (this week)
A checklist of 3-8 improvements (idempotency, replay protection, better gas strategy, queue backoff, signature rotation, monitoring).

## UI Badges & Alerts
### Badges
- <badge1>
- <badge2>

### Alerts
- **Info:** <message> (trigger: <condition>)
- **Warning:** <message> (trigger: <condition>)
- **Error:** <message> (trigger: <condition>)

RULES:
- Ignore the word "Simulated" wherever it appears; treat events as real.
- Prefer the event timeline order + final "settled/refunded" evidence when lifecycleStage list conflicts.
- If mint+burn happened but Ethereum settle failed AND a refund is shown: Final Status = settled_failure_refunded.
- If mint happened but burn is missing: flag possible double-mint / unbacked token risk.
- If Ethereum lock is present but neither settle nor refund is present: flag possible stuck-funds risk.
- Compute ETH estimate as wei / 1e18 with 6 decimals (no fiat).
- If timestamps are missing or same, write "n/a" and say why."#;

async fn analyze_transaction(
    State(state): State<Arc<AppState>>,
    Path(nonce): Path<u64>,
) -> Result<impl IntoResponse, StatusCode> {
    let openai_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| {
            error!("OPENAI_API_KEY not set");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    // Fetch transaction + events + proof
    let msg = db::get_message_by_nonce(&state.pool, nonce)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let events = db::get_events_by_nonce(&state.pool, nonce)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let proof = msg.proof_json.as_ref().and_then(|json| {
        serde_json::from_str::<crate::types::ProofBundle>(json).ok()
    });

    // Build the JSON payload for the prompt
    let tx_data = serde_json::json!({
        "txId": msg.trace_id,
        "nonce": msg.nonce,
        "humanLabel": msg.description,
        "sender": msg.sender,
        "amountWei": msg.amount,
        "state": msg.state,
        "lifecycleStages": events.iter().map(|e| serde_json::json!({
            "stage": e.step,
            "chain": e.actor,
            "status": e.status,
            "timestamp": e.timestamp,
            "details": e.detail,
        })).collect::<Vec<_>>(),
        "proofBundle": proof.as_ref().map(|p| serde_json::json!({
            "blockHeaderHash": p.block_header,
            "eventRootHash": p.event_root,
            "ecdsaSignature": p.validator_signature,
            "relayerSigner": p.relayer_address,
            "merkleNodes": p.inclusion_proof,
        })),
        "references": {
            "ethLockTx": events.iter().find(|e| e.step == crate::event::Step::Locked).and_then(|e| e.detail.clone()),
            "ethSettleTx": msg.eth_settle_tx,
            "solanaExecuteSig": msg.solana_signature,
        },
        "counters": {
            "retries": msg.retry_count,
        },
        "flags": {
            "pending": msg.state == "observed" || msg.state == "persisted" || msg.state == "verified" || msg.state == "sent_to_solana" || msg.state == "executed",
            "failed": msg.state == "failed",
            "rollbackTriggered": msg.state == "rolled_back",
        },
    });

    let user_msg = format!("NOW ANALYZE THIS DATA:\n{}", serde_json::to_string_pretty(&tx_data).unwrap_or_default());

    // Call OpenAI
    let client = reqwest::Client::new();
    let openai_res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", openai_key))
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "system", "content": ANALYSIS_SYSTEM_PROMPT },
                { "role": "user", "content": user_msg },
            ],
            "temperature": 0.3,
            "max_tokens": 4000,
        }))
        .send()
        .await
        .map_err(|e| {
            error!(error = %e, "OpenAI request failed");
            StatusCode::BAD_GATEWAY
        })?;

    if !openai_res.status().is_success() {
        let status = openai_res.status();
        let body = openai_res.text().await.unwrap_or_default();
        error!(%status, %body, "OpenAI returned error");
        return Err(StatusCode::BAD_GATEWAY);
    }

    let body: serde_json::Value = openai_res.json().await.map_err(|e| {
        error!(error = %e, "Failed to parse OpenAI response");
        StatusCode::BAD_GATEWAY
    })?;

    let analysis = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("Analysis unavailable")
        .to_string();

    Ok(Json(serde_json::json!({
        "nonce": nonce,
        "analysis": analysis,
    })))
}

// ──────────────────────────────────────────────
// WebSocket Handler
// ──────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to the event broadcast channel
    let mut event_rx = state.event_tx.subscribe();

    info!("WebSocket client connected");

    // Send existing events as initial state
    if let Ok(messages) = db::get_all_messages(&state.pool).await {
        for msg in messages.iter().take(100) {
            if let Ok(events) = db::get_events_by_nonce(&state.pool, msg.nonce as u64).await {
                for event in events {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if sender.send(Message::Text(json)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }

    // Forward broadcast events to the WebSocket client
    let send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match serde_json::to_string(&event) {
                Ok(json) => {
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to serialize event");
                }
            }
        }
    });

    // Handle incoming messages (for future use, e.g., subscriptions)
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(_) => {} // Ignore other messages for now
                Err(_) => break,
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    info!("WebSocket client disconnected");
}
