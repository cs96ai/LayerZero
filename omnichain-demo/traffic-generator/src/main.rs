use anyhow::Result;
use clap::Parser;
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use rand::Rng;
use rand::seq::SliceRandom;
use std::str::FromStr;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};
use uuid::Uuid;

/// Synthetic traffic generator for the omnichain escrow demo.
/// Submits randomized escrow requests to the Ethereum contract.
#[derive(Parser, Debug)]
#[command(name = "traffic-generator")]
#[command(about = "Generate synthetic cross-chain escrow requests")]
struct Args {
    /// Requests per second
    #[arg(short, long, default_value_t = 1.0)]
    rate: f64,

    /// Total number of requests (0 = unlimited)
    #[arg(short, long, default_value_t = 0)]
    count: u64,

    /// Ethereum RPC URL
    #[arg(long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,

    /// Escrow contract address
    #[arg(long, default_value = "0x5FbDB2315678afecb367f032d93F642f64180aa3")]
    escrow_address: String,

    /// Number of simulated users (uses Anvil default accounts)
    #[arg(long, default_value_t = 5)]
    users: usize,

    /// Minimum lock amount in wei
    #[arg(long, default_value_t = 100000)]
    min_amount: u64,

    /// Maximum lock amount in wei
    #[arg(long, default_value_t = 1000000)]
    max_amount: u64,

    /// Demo scenario: "steady" | "burst" | "failures"
    #[arg(long, default_value = "steady")]
    scenario: String,
}

// Anvil default private keys (accounts 1-9, account 0 is the relayer)
// Human-readable names for simulated users (mapped to Anvil accounts 1-9)
const USER_NAMES: &[&str] = &[
    "Alice", "Bob", "Charlie", "Diana", "Eve", "Frank", "Grace", "Hank", "Ivy",
];

// Fun payment descriptions
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

const ANVIL_KEYS: &[&str] = &[
    "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d", // account 1
    "5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a", // account 2
    "7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6", // account 3
    "47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a", // account 4
    "8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba", // account 5
    "92db14e403b83dfe3df233f83dfa3a0d7096f21ca9b0d6d6b8d88b2b4ec1564e", // account 6
    "4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356", // account 7
    "dbda1821b80551c9d65939329250298aa3472ba22feea921c0cf5d620ea67b97", // account 8
    "2a871d0798f97d79848a013d4936a73bf4cc922c825d33c1cf7073dff6d409c6", // account 9
];

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "traffic_generator=info".into()),
        )
        .init();

    let args = Args::parse();
    info!(?args, "Starting traffic generator");

    let interval = Duration::from_secs_f64(1.0 / args.rate);
    let user_count = args.users.min(ANVIL_KEYS.len());

    // Build signer clients for each simulated user
    let provider = Provider::<Http>::try_from(&args.rpc_url)?;
    let chain_id = provider.get_chainid().await?.as_u64();

    let wallets: Vec<LocalWallet> = ANVIL_KEYS[..user_count]
        .iter()
        .map(|key| {
            key.parse::<LocalWallet>()
                .unwrap()
                .with_chain_id(chain_id)
        })
        .collect();

    info!(
        users = user_count,
        chain_id,
        rate = args.rate,
        scenario = %args.scenario,
        "Traffic generator ready"
    );

    let contract_address = Address::from_str(&args.escrow_address)?;

    let mut sent: u64 = 0;
    let mut rng = rand::thread_rng();

    loop {
        if args.count > 0 && sent >= args.count {
            info!(total = sent, "Reached target count, stopping");
            break;
        }

        // Pick a random user
        let wallet_idx = rng.gen_range(0..user_count);
        let wallet = wallets[wallet_idx].clone();

        // Generate random payload with a human-readable description
        let trace_id = Uuid::new_v4();
        let amount = rng.gen_range(args.min_amount..=args.max_amount);
        let user_name = USER_NAMES[wallet_idx];
        let action = PAYMENT_ACTIONS.choose(&mut rng).unwrap();
        let recipient_name = USER_NAMES.choose(&mut rng).unwrap();
        let description = format!("{}'s payment to {} for {}", user_name, recipient_name, action);
        let payload = generate_payload(&mut rng, &trace_id, &description);

        // Apply scenario modifiers
        let effective_interval = match args.scenario.as_str() {
            "burst" => {
                if sent % 10 < 3 {
                    Duration::from_millis(50) // burst of 3 rapid requests every 10
                } else {
                    interval
                }
            }
            "failures" => {
                // Occasionally send with 0 value to trigger revert (for testing error handling)
                if rng.gen_ratio(1, 5) {
                    info!(nonce = sent + 1, "Injecting failure scenario (zero value)");
                    // We'll handle this below
                    interval
                } else {
                    interval
                }
            }
            _ => interval, // "steady"
        };

        // Build and send transaction
        let provider = Provider::<Http>::try_from(&args.rpc_url)?;
        let client = SignerMiddleware::new(provider, wallet);

        // lockFunds(bytes payload) — function selector
        let selector = &ethers::utils::keccak256(b"lockFunds(bytes)")[..4];
        let encoded = ethers::abi::encode(&[ethers::abi::Token::Bytes(payload.clone())]);
        let mut calldata = selector.to_vec();
        calldata.extend_from_slice(&encoded);

        let effective_amount = if args.scenario == "failures" && rng.gen_ratio(1, 10) {
            0u64 // This will trigger ZeroValue revert
        } else {
            amount
        };

        let tx = TransactionRequest::new()
            .to(contract_address)
            .data(calldata)
            .value(effective_amount)
            .gas(200_000u64);

        match client.send_transaction(tx, None).await {
            Ok(pending) => {
                let tx_hash = pending.tx_hash();
                match pending.await {
                    Ok(Some(receipt)) => {
                        sent += 1;
                        info!(
                            seq = sent,
                            %tx_hash,
                            %description,
                            amount = effective_amount,
                            trace_id = %trace_id,
                            status = ?receipt.status,
                            "Transaction confirmed"
                        );
                    }
                    Ok(None) => {
                        warn!(%tx_hash, "Transaction dropped");
                    }
                    Err(e) => {
                        warn!(error = %e, "Transaction failed");
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to send transaction");
            }
        }

        sleep(effective_interval).await;
    }

    info!(total = sent, "Traffic generation complete");
    Ok(())
}

/// Generate a randomized payload with embedded trace information and description.
fn generate_payload(rng: &mut impl Rng, trace_id: &Uuid, description: &str) -> Vec<u8> {
    let mut payload = Vec::new();

    // Embed trace ID as first 16 bytes
    payload.extend_from_slice(trace_id.as_bytes());

    // Embed description length (2 bytes) + description bytes
    let desc_bytes = description.as_bytes();
    payload.extend_from_slice(&(desc_bytes.len() as u16).to_be_bytes());
    payload.extend_from_slice(desc_bytes);

    // Add some random operation data (4-16 bytes)
    let extra_len = rng.gen_range(4..=16);
    let mut extra = vec![0u8; extra_len];
    rng.fill(&mut extra[..]);
    payload.extend_from_slice(&extra);

    payload
}
