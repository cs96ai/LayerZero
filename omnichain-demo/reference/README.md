# Reference Implementations

This directory contains reference implementations that are **not executed** in the deployed demo but document what the real on-chain counterparts would look like.

## `solana-program/`

A fully implemented Solana BPF program (`cross-chain-executor`) that:

- Receives cross-chain execution requests via Borsh-serialized instructions
- Derives PDA receipt accounts keyed by nonce for idempotency / replay protection
- Performs the deterministic computation (`amount × 2`) matching the simulation stub
- Writes an `ExecutionReceipt` with nonce, result, sender, trace_id, and timestamp
- Emits structured `EVENT:{...}` logs for relayer observability

The deployed demo uses a simulation stub (`relayer/src/solana_sim.rs`) that produces identical deterministic results without requiring a running Solana validator. This keeps the Docker image lightweight and avoids the ~1GB Solana toolchain dependency.

### To run locally with the real program

```bash
# Start solana-test-validator
solana-test-validator &

# Build and deploy
cd reference/solana-program
cargo build-sbf
solana program deploy target/deploy/cross_chain_executor.so
```
