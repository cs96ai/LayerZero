use anyhow::Result;
use ethers::prelude::*;
use ethers::types::{Address, Filter, Log, H256, U256};
use std::str::FromStr;
use tracing::{debug, info, warn};

/// Parsed CrossChainRequest event from the Ethereum escrow contract.
#[derive(Debug, Clone)]
pub struct CrossChainRequestEvent {
    pub trace_id: H256,
    pub nonce: u64,
    pub sender: Address,
    pub amount: U256,
    pub payload: Vec<u8>,
    pub deadline: U256,
    pub block_number: u64,
    pub tx_hash: H256,
}

/// Compute the event topic hash for CrossChainRequest.
pub fn event_signature() -> H256 {
    // keccak256("CrossChainRequest(bytes32,uint64,address,uint256,bytes,uint256)")
    let hash = ethers::utils::keccak256(
        b"CrossChainRequest(bytes32,uint64,address,uint256,bytes,uint256)",
    );
    H256::from(hash)
}

/// Build a log filter for CrossChainRequest events from a given block.
pub fn build_filter(escrow_address: &str, from_block: u64) -> Result<Filter> {
    let address = Address::from_str(escrow_address)?;
    let topic = event_signature();

    Ok(Filter::new()
        .address(address)
        .topic0(topic)
        .from_block(from_block))
}

/// Parse a raw log into a CrossChainRequestEvent.
pub fn parse_log(log: &Log) -> Result<CrossChainRequestEvent> {
    // topic[0] = event signature
    // topic[1] = traceId (indexed bytes32)
    // topic[2] = nonce (indexed uint64, padded to 32 bytes)
    let trace_id = log.topics.get(1).copied().unwrap_or_default();

    let nonce_bytes = log.topics.get(2).copied().unwrap_or_default();
    let nonce = U256::from_big_endian(nonce_bytes.as_bytes()).as_u64();

    // data = abi.encode(address sender, uint256 amount, bytes payload, uint256 deadline)
    let data = &log.data.0;
    if data.len() < 128 {
        anyhow::bail!("Log data too short: {} bytes", data.len());
    }

    // sender is at offset 0, right-padded in 32 bytes
    let sender = Address::from_slice(&data[12..32]);

    // amount is at offset 32
    let amount = U256::from_big_endian(&data[32..64]);

    // payload offset is at offset 64 (pointer to dynamic data)
    let payload_offset = U256::from_big_endian(&data[64..96]).as_usize();
    let payload_len = U256::from_big_endian(&data[payload_offset..payload_offset + 32]).as_usize();
    let payload = data[payload_offset + 32..payload_offset + 32 + payload_len].to_vec();

    // deadline is at offset 96
    let deadline = U256::from_big_endian(&data[96..128]);

    let block_number = log.block_number.map(|b| b.as_u64()).unwrap_or(0);
    let tx_hash = log.transaction_hash.unwrap_or_default();

    debug!(
        nonce,
        ?sender,
        %amount,
        %deadline,
        "Parsed CrossChainRequest event"
    );

    Ok(CrossChainRequestEvent {
        trace_id,
        nonce,
        sender,
        amount,
        payload,
        deadline,
        block_number,
        tx_hash,
    })
}

/// Fetch logs from Ethereum RPC.
pub async fn fetch_logs(rpc_url: &str, escrow_address: &str, from_block: u64) -> Result<Vec<Log>> {
    let provider = Provider::<Http>::try_from(rpc_url)?;
    let filter = build_filter(escrow_address, from_block)?;
    let logs = provider.get_logs(&filter).await?;
    info!(count = logs.len(), from_block, "Fetched Ethereum logs");
    Ok(logs)
}

/// Get the current block number.
pub async fn get_block_number(rpc_url: &str) -> Result<u64> {
    let provider = Provider::<Http>::try_from(rpc_url)?;
    let block = provider.get_block_number().await?;
    Ok(block.as_u64())
}

/// Sign a settlement message: keccak256(abi.encodePacked(nonce, result))
/// Returns the 65-byte signature.
pub fn sign_settlement(private_key: &str, nonce: u64, result: &[u8]) -> Result<Vec<u8>> {
    use ethers::signers::LocalWallet;

    let wallet: LocalWallet = private_key.parse()?;

    // Match the contract: keccak256(abi.encodePacked(nonce, result))
    let mut msg = Vec::new();
    msg.extend_from_slice(&nonce.to_be_bytes());
    msg.extend_from_slice(result);
    let hash = ethers::utils::keccak256(&msg);

    // eth_sign style: "\x19Ethereum Signed Message:\n32" + hash
    let signature = wallet.sign_hash(H256::from(hash))?;
    let sig_bytes = signature.to_vec();

    Ok(sig_bytes)
}

/// Call settle() on the escrow contract.
/// Returns the transaction hash.
pub async fn call_settle(
    rpc_url: &str,
    private_key: &str,
    escrow_address: &str,
    nonce: u64,
    result: &[u8],
    signature: &[u8],
) -> Result<H256> {
    use ethers::abi::Token;
    use ethers::signers::{LocalWallet, Signer};

    let provider = Provider::<Http>::try_from(rpc_url)?;
    let wallet: LocalWallet = private_key.parse()?;
    let client = SignerMiddleware::new(provider, wallet.with_chain_id(31337u64));

    let contract_address = Address::from_str(escrow_address)?;

    // ABI encode: settle(uint64 _nonce, bytes result, bytes signature)
    let selector = &ethers::utils::keccak256(b"settle(uint64,bytes,bytes)")[..4];
    let encoded = ethers::abi::encode(&[
        Token::Uint(U256::from(nonce)),
        Token::Bytes(result.to_vec()),
        Token::Bytes(signature.to_vec()),
    ]);

    let mut calldata = selector.to_vec();
    calldata.extend_from_slice(&encoded);

    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(calldata)
        .gas(500_000u64);

    let pending = client.send_transaction(tx, None).await?;
    let tx_hash = pending.tx_hash();

    info!(%tx_hash, nonce, "Settlement transaction sent");

    // Wait for confirmation
    let receipt = pending.await?;
    match receipt {
        Some(r) => {
            info!(
                tx_hash = %r.transaction_hash,
                status = ?r.status,
                "Settlement confirmed"
            );
            Ok(r.transaction_hash)
        }
        None => {
            warn!(nonce, "Settlement tx dropped");
            anyhow::bail!("Settlement transaction was dropped")
        }
    }
}
