use anyhow::Result;
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use rand::seq::SliceRandom;
use rand::Rng;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::types::AppState;

// Human-readable names for simulated users (mapped to Anvil accounts 1-9)
const USER_NAMES: &[&str] = &[
    "Alice", "Bob", "Charlie", "Diana", "Eve", "Frank", "Grace", "Hank", "Ivy",
];

const PAYMENT_ACTIONS: &[&str] = &[
    "shovelling the driveway",
    "dog walking",
    "freelance web design",
    "car detailing",
    "guitar lessons",
    "birthday cake order",
    "lawn mowing",
    "tutoring session",
    "photography gig",
    "catering deposit",
    "house painting estimate",
    "yoga class pack",
    "vintage record collection",
    "roof repair quote",
    "moving truck rental",
    "wedding DJ deposit",
    "pottery class",
    "piano tuning",
    "pet sitting",
    "snow plowing",
    "boat detailing",
    "art commission",
    "personal training",
    "babysitting",
    "handyman services",
    "meal prep delivery",
    "tailoring alterations",
    "math tutoring",
    "pool cleaning",
    "window washing",
];

// Anvil default private keys (accounts 1-5, account 0 is the relayer)
const ANVIL_KEYS: &[&str] = &[
    "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
    "5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a",
    "7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6",
    "47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a",
    "8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba",
];

/// Background task that generates traffic when simulation_running is true.
/// Checks the deadline and auto-stops when expired.
pub async fn run_traffic_generator(state: Arc<AppState>, rpc_url: String, escrow_address: String) {
    info!("Traffic generator task started (waiting for simulation start)");

    loop {
        // Wait until simulation is running
        if !state.simulation_running.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        // Check deadline
        let deadline = state.simulation_deadline.load(Ordering::Relaxed);
        if deadline > 0 {
            let now = chrono::Utc::now().timestamp();
            if now >= deadline {
                info!("Simulation deadline reached, auto-stopping");
                state.simulation_running.store(false, Ordering::Relaxed);
                state.paused.store(true, Ordering::Relaxed);
                continue;
            }
        }

        // Generate one transaction
        if let Err(e) = send_one_transaction(&rpc_url, &escrow_address).await {
            warn!(error = %e, "Traffic generator: failed to send transaction");
        }

        // 1 transaction every 5 seconds
        sleep(Duration::from_secs(5)).await;
    }
}

async fn send_one_transaction(rpc_url: &str, escrow_address: &str) -> Result<()> {
    // Generate all random values upfront so rng doesn't live across await points
    let (wallet_idx, description, trace_id, amount, payload) = {
        let mut rng = rand::thread_rng();
        let wallet_idx = rng.gen_range(0..ANVIL_KEYS.len());
        let user_name = USER_NAMES[wallet_idx];
        let action = *PAYMENT_ACTIONS.choose(&mut rng).unwrap();
        let recipient_name = *USER_NAMES.choose(&mut rng).unwrap();
        let description = format!("{}'s payment to {} for {}", user_name, recipient_name, action);
        let trace_id = Uuid::new_v4();
        let amount: u64 = rng.gen_range(100_000..=1_000_000);
        let payload = generate_payload(&mut rng, &trace_id, &description);
        (wallet_idx, description, trace_id, amount, payload)
    };

    let provider = Provider::<Http>::try_from(rpc_url)?;
    let chain_id = provider.get_chainid().await?.as_u64();

    let wallet: LocalWallet = ANVIL_KEYS[wallet_idx]
        .parse::<LocalWallet>()?
        .with_chain_id(chain_id);

    let contract_address = Address::from_str(escrow_address)?;
    let client = SignerMiddleware::new(provider, wallet);

    let selector = &ethers::utils::keccak256(b"lockFunds(bytes)")[..4];
    let encoded = ethers::abi::encode(&[ethers::abi::Token::Bytes(payload)]);
    let mut calldata = selector.to_vec();
    calldata.extend_from_slice(&encoded);

    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(calldata)
        .value(amount)
        .gas(500_000u64);

    match client.send_transaction(tx, None).await {
        Ok(pending) => {
            let tx_hash = pending.tx_hash();
            match pending.await {
                Ok(Some(receipt)) => {
                    info!(
                        %tx_hash,
                        %description,
                        amount,
                        trace_id = %trace_id,
                        status = ?receipt.status,
                        "Traffic: transaction confirmed"
                    );
                }
                Ok(None) => warn!(%tx_hash, "Traffic: transaction dropped"),
                Err(e) => warn!(error = %e, "Traffic: transaction failed"),
            }
        }
        Err(e) => {
            error!(error = %e, "Traffic: failed to send transaction");
        }
    }

    Ok(())
}

fn generate_payload(rng: &mut impl Rng, trace_id: &Uuid, description: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(trace_id.as_bytes());
    let desc_bytes = description.as_bytes();
    payload.extend_from_slice(&(desc_bytes.len() as u16).to_be_bytes());
    payload.extend_from_slice(desc_bytes);
    let extra_len = rng.gen_range(4..=16);
    let mut extra = vec![0u8; extra_len];
    rng.fill(&mut extra[..]);
    payload.extend_from_slice(&extra);
    payload
}
