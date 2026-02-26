use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

// ──────────────────────────────────────────────
// Program entrypoint
// ──────────────────────────────────────────────

entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = CrossChainInstruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    match instruction {
        CrossChainInstruction::ExecuteCrossChain {
            nonce,
            sender,
            amount,
            payload,
            trace_id,
        } => execute_cross_chain(program_id, accounts, nonce, sender, amount, payload, trace_id),
    }
}

// ──────────────────────────────────────────────
// Instruction enum
// ──────────────────────────────────────────────

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum CrossChainInstruction {
    /// Execute a cross-chain request from Ethereum.
    ///
    /// Accounts expected:
    /// 0. `[signer, writable]` Payer (relayer)
    /// 1. `[writable]` Receipt PDA account
    /// 2. `[]` System program
    ExecuteCrossChain {
        nonce: u64,
        sender: [u8; 20], // Ethereum address
        amount: u64,       // in wei-equivalent units (scaled down for Solana)
        payload: Vec<u8>,
        trace_id: [u8; 32],
    },
}

// ──────────────────────────────────────────────
// Receipt account data
// ──────────────────────────────────────────────

/// SIMULATION: This receipt record acts as a non-transferable proof of execution.
/// In a real system, this would be an SPL token mint with transfer restrictions.
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ExecutionReceipt {
    /// Marks this account as initialized
    pub is_initialized: bool,
    /// Ethereum escrow nonce
    pub nonce: u64,
    /// Deterministic computation result (amount * 2)
    pub result: u64,
    /// Original Ethereum sender
    pub sender: [u8; 20],
    /// Trace ID for observability
    pub trace_id: [u8; 32],
    /// Unix timestamp of execution
    pub executed_at: i64,
}

impl ExecutionReceipt {
    pub const SIZE: usize = 1 + 8 + 8 + 20 + 32 + 8; // 77 bytes
}

// ──────────────────────────────────────────────
// Seeds for PDA derivation
// ──────────────────────────────────────────────

pub const RECEIPT_SEED: &[u8] = b"receipt";

pub fn find_receipt_pda(program_id: &Pubkey, nonce: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[RECEIPT_SEED, &nonce.to_le_bytes()], program_id)
}

// ──────────────────────────────────────────────
// Instruction handler
// ──────────────────────────────────────────────

fn execute_cross_chain(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    nonce: u64,
    sender: [u8; 20],
    amount: u64,
    _payload: Vec<u8>,
    trace_id: [u8; 32],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let payer = next_account_info(accounts_iter)?;
    let receipt_account = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    // Verify payer is signer
    if !payer.is_signer {
        msg!("ERROR: Payer must be a signer");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Derive and verify receipt PDA
    let (expected_pda, bump) = find_receipt_pda(program_id, nonce);
    if *receipt_account.key != expected_pda {
        msg!("ERROR: Invalid receipt PDA");
        return Err(ProgramError::InvalidArgument);
    }

    // Check if receipt already exists (idempotency / replay protection)
    if receipt_account.data_len() > 0 && receipt_account.lamports() > 0 {
        let existing = ExecutionReceipt::try_from_slice(&receipt_account.data.borrow())
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if existing.is_initialized {
            msg!(
                "WARN: Receipt for nonce {} already exists, skipping (idempotent)",
                nonce
            );
            // Emit event log even on skip for observability
            emit_event_log(&trace_id, nonce, "executed", "success", "idempotent-skip");
            return Ok(());
        }
    }

    // ── Deterministic computation ──
    // SIMULATION: The "cross-chain logic" is a simple deterministic function.
    // In a real system this could be any arbitrary computation.
    let result = amount.checked_mul(2).ok_or(ProgramError::ArithmeticOverflow)?;

    msg!("Cross-chain execution: nonce={}, amount={}, result={}", nonce, amount, result);

    // ── Create receipt PDA account ──
    let receipt_size = ExecutionReceipt::SIZE;
    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(receipt_size);

    let seeds: &[&[u8]] = &[RECEIPT_SEED, &nonce.to_le_bytes(), &[bump]];

    invoke(
        &system_instruction::create_account(
            payer.key,
            receipt_account.key,
            lamports,
            receipt_size as u64,
            program_id,
        ),
        &[payer.clone(), receipt_account.clone(), system_program.clone()],
    )?;

    // Write receipt data
    // SIMULATION: This receipt acts as a non-transferable mint record.
    // In a real bridge, this would be an SPL token with freeze authority.
    let clock = solana_program::clock::Clock::get()?;
    let receipt = ExecutionReceipt {
        is_initialized: true,
        nonce,
        result,
        sender,
        trace_id,
        executed_at: clock.unix_timestamp,
    };

    receipt.serialize(&mut &mut receipt_account.data.borrow_mut()[..])?;

    // ── Emit structured execution log ──
    emit_event_log(&trace_id, nonce, "executed", "success", "receipt-created");

    // SIMULATION: Emit a "minted" event to represent the bridged receipt token
    emit_event_log(&trace_id, nonce, "minted", "success", "receipt-minted");

    msg!(
        "Receipt created: nonce={}, result={}, pda={}",
        nonce,
        result,
        receipt_account.key
    );

    Ok(())
}

// ──────────────────────────────────────────────
// Structured event logging
// ──────────────────────────────────────────────

/// Emit a structured log that the relayer can parse.
/// Format: EVENT:{"trace_id":"...","nonce":N,"actor":"solana","step":"...","status":"..."}
fn emit_event_log(trace_id: &[u8; 32], nonce: u64, step: &str, status: &str, detail: &str) {
    let trace_hex: String = trace_id.iter().map(|b| format!("{:02x}", b)).collect();
    msg!(
        "EVENT:{{\"trace_id\":\"{}\",\"nonce\":{},\"actor\":\"solana\",\"step\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\"}}",
        trace_hex,
        nonce,
        step,
        status,
        detail
    );
}
