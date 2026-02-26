use anyhow::Result;
use ethers::signers::{LocalWallet, Signer};
use ethers::types::H256;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::types::ProofBundle;

/// Semi-real verification model using ECDSA signatures.
///
/// Upgrade path from pure simulation:
/// - Block header and event root are derived from real SHA-256 hashes of the data
/// - Merkle inclusion proof nodes are deterministic (seeded by nonce), not random
/// - Validator signature is a **real ECDSA signature** over keccak256(block_header || event_root || nonce)
/// - Verification uses ecrecover to check the signer matches the relayer's address
///
/// This is the "Validator Signature" approach used by production bridges like
/// early Wormhole and Ronin — real cryptography, one library call.

/// Generate a proof bundle with real ECDSA signature.
pub fn generate_proof_bundle(
    nonce: u64,
    block_number: u64,
    tx_hash: &str,
    event_data: &[u8],
    relayer_private_key: &str,
) -> Result<ProofBundle> {
    // Deterministic block header hash from real data
    let block_header = {
        let mut hasher = Sha256::new();
        hasher.update(b"block_header:");
        hasher.update(block_number.to_le_bytes());
        hasher.update(tx_hash.as_bytes());
        hex::encode(hasher.finalize())
    };

    // Deterministic event root from real event data
    let event_root = {
        let mut hasher = Sha256::new();
        hasher.update(b"event_root:");
        hasher.update(event_data);
        hex::encode(hasher.finalize())
    };

    // Deterministic Merkle inclusion proof (3 sibling hashes, seeded by nonce)
    let inclusion_proof: Vec<String> = (0..3)
        .map(|i| {
            let mut hasher = Sha256::new();
            hasher.update(b"proof_node:");
            hasher.update(i.to_string().as_bytes());
            hasher.update(nonce.to_le_bytes());
            hasher.update(event_data);
            hex::encode(hasher.finalize())
        })
        .collect();

    // REAL ECDSA: Sign keccak256(block_header || event_root || nonce) with relayer key
    let message = compute_signing_message(&block_header, &event_root, nonce);
    let wallet: LocalWallet = relayer_private_key.parse()?;
    let signature = wallet.sign_hash(H256::from(message))?;
    let validator_signature = hex::encode(signature.to_vec());

    let relayer_address = format!("{:?}", wallet.address());

    info!(
        nonce,
        block_number,
        %relayer_address,
        "Generated ECDSA-signed proof bundle"
    );

    Ok(ProofBundle {
        block_header,
        event_root,
        inclusion_proof,
        validator_signature,
        relayer_address,
        nonce,
        verified: false,
    })
}

/// Verify a proof bundle using real ECDSA signature recovery.
///
/// 1. Recompute the message hash from block_header, event_root, nonce
/// 2. Recover the signer address from the signature
/// 3. Check the recovered address matches the claimed relayer address
pub fn verify_proof_bundle(proof: &ProofBundle) -> Result<bool> {
    // Structural checks
    if proof.block_header.is_empty() {
        anyhow::bail!("Missing block header");
    }
    if proof.event_root.is_empty() {
        anyhow::bail!("Missing event root");
    }
    if proof.inclusion_proof.is_empty() {
        anyhow::bail!("Missing inclusion proof");
    }
    if proof.validator_signature.is_empty() {
        anyhow::bail!("Missing validator signature");
    }
    if proof.nonce == 0 {
        anyhow::bail!("Invalid nonce in proof bundle");
    }

    // REAL ECDSA: Recover signer from signature and verify it matches relayer_address
    let message = compute_signing_message(&proof.block_header, &proof.event_root, proof.nonce);
    let sig_bytes = hex::decode(&proof.validator_signature)?;
    let signature = ethers::types::Signature::try_from(sig_bytes.as_slice())?;
    let recovered = signature.recover(H256::from(message))?;
    let recovered_str = format!("{:?}", recovered);

    if recovered_str.to_lowercase() != proof.relayer_address.to_lowercase() {
        anyhow::bail!(
            "ECDSA verification failed: recovered {} but expected {}",
            recovered_str,
            proof.relayer_address
        );
    }

    info!(
        nonce = proof.nonce,
        %recovered_str,
        "Proof bundle verified (ECDSA)"
    );
    Ok(true)
}

/// Compute the message to sign: keccak256(block_header || event_root || nonce)
fn compute_signing_message(block_header: &str, event_root: &str, nonce: u64) -> [u8; 32] {
    let mut data = Vec::new();
    data.extend_from_slice(block_header.as_bytes());
    data.extend_from_slice(event_root.as_bytes());
    data.extend_from_slice(&nonce.to_be_bytes());
    ethers::utils::keccak256(&data)
}
