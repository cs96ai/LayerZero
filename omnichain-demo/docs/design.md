# Design Document — Omnichain Escrow Demo

## Safety vs Liveness

Cross-chain systems face an inherent tension between **safety** (never producing an incorrect result) and **liveness** (always eventually producing a result).

### Safety Guarantees in This System
- **Replay protection**: Each nonce can only be settled once. The Ethereum contract enforces `!escrow.executed` before releasing funds.
- **Timeout reclaim**: If the relayer fails or Solana is unreachable, the sender can reclaim funds after the deadline.
- **Idempotent processing**: The relayer's state machine ensures each message is processed exactly once, even after crashes.

### Liveness Guarantees
- **Crash-safe resume**: The relayer persists state to SQLite before advancing. On restart, it resumes from the last persisted state.
- **Retry logic**: Transient failures (RPC timeouts, tx reverts) trigger retries with backoff.
- **Timeout fallback**: If liveness fails entirely, the timeout mechanism on Ethereum ensures funds are not permanently locked.

### Tradeoff
This system **prioritizes safety over liveness**. It is acceptable for a transaction to be delayed or to time out, but it must never double-settle or lose funds.

## Why Atomicity Is Impossible Cross-Chain

Traditional database transactions provide ACID guarantees within a single system. Cross-chain operations span two independent consensus systems with no shared state.

### The Fundamental Problem
1. Ethereum and Solana have independent finality
2. No single transaction can atomically span both chains
3. A commit on one chain cannot be rolled back if the other chain fails

### Consequences
- We must use a **two-phase commit analog**: lock on source → execute on destination → settle on source
- Between phases, the system is in an **intermediate state** where funds are locked but not settled
- This intermediate state must be handled correctly: timeouts, retries, and idempotency are essential

### Our Approach
We use an **escrow pattern** that is safe under partial failure:
- Funds locked on Ethereum with a deadline
- If settlement succeeds → funds released
- If settlement fails or times out → sender reclaims
- The relayer is the coordinator, but the escrow contract is the source of truth

## Event-Driven Architecture Rationale

### Why Events?
1. **Decoupling**: Each component (Ethereum, Solana, relayer, dashboard) operates independently and communicates through events
2. **Traceability**: Every state transition is recorded as an event with a trace ID, enabling full lifecycle reconstruction
3. **Observability**: The dashboard can visualize the entire system state by consuming the event stream
4. **Replay**: Events can be replayed for debugging, testing, or demonstration purposes

### Event Flow
```
Ethereum (emit event) → Relayer (observe, persist, forward) → Solana (execute, log)
                                    ↓
                              WebSocket stream → Dashboard
```

### Why WebSocket?
- Real-time updates without polling
- Low latency for dashboard visualization
- Natural fit for event streaming

## Simulated Components

### Simulated Bridge
**What's real**: Funds are locked in a Solidity escrow contract. Settlement requires a valid ECDSA signature verified on-chain.
**What's simulated**: Solana execution runs as a local simulation stub that produces the same deterministic result (`amount × 2`) as the real Solana program. No actual tokens cross chain boundaries. See `reference/solana-program/` for the full on-chain Solana program implementation (PDA receipt accounts, borsh serialization, idempotency, structured event logs).

### ECDSA-Signed Verification
**What's real**: The relayer signs proof bundles with a real ECDSA private key over `keccak256(block_header || event_root || nonce)`. Verification uses `ecrecover` to check the signer matches the relayer's address. The Ethereum `settle()` function independently verifies the signature on-chain. Block headers and event roots are SHA-256 hashes of real data.
**What's simulated**: Merkle inclusion proof nodes are deterministic (seeded by nonce) rather than derived from an actual Merkle tree. The validator is a single relayer key, not a threshold signature from a validator set.

### Simulation Boundary Documentation
Every simulated component is marked with `// SIMULATION:` comments in the source code explaining what is simulated and what a real implementation would require.

## Future Improvements

1. **Multi-relayer consensus**: Replace single trusted relayer with a committee using threshold signatures
2. **Real light-client verification**: Integrate actual block header verification (e.g., Ethereum light client on Solana)
3. **SPL token minting**: Replace receipt records with real SPL token minting/burning
4. **MEV protection**: Add commit-reveal or encrypted mempool for settlement transactions
5. **Multi-chain support**: Extend beyond Ethereum↔Solana to arbitrary chain pairs
6. **Formal verification**: Model the escrow state machine in TLA+ or similar
7. **Gas optimization**: Batch multiple settlements into single transactions
8. **Monitoring & alerting**: Production-grade observability with Prometheus/Grafana
