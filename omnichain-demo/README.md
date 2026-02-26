# Omnichain Escrow Demo

A local research prototype demonstrating cross-chain coordination between Ethereum and Solana with a Rust relayer and a React global observability dashboard.

> **All data is generated locally. No real assets or external services.**

## Architecture Overview

```
┌──────────────┐     ┌───────────┐     ┌ ─ ─ ─ ─ ─ ─ ─┐
│   Ethereum   │────▶│   Rust    │ ─ ▶   Solana       
│  (Anvil)     │◀────│  Relayer  │      (simulated)   │
│              │     │           │     │               
│ CrossChain   │     │ State     │      see reference/ │
│ Escrow.sol   │     │ Machine   │     │               
└──────────────┘     └─────┬─────┘     └ ─ ─ ─ ─ ─ ─ ─┘
                           │
                    ┌──────┴──────┐
                    │   React     │
                    │  Dashboard  │
                    │  (WS+HTTP)  │
                    └─────────────┘
```

## Trust Model

- **Single trusted relayer**: The relayer is the sole bridge between chains. It is trusted to faithfully relay messages and results.
- **No decentralized validation**: This is a research prototype; there is no committee, oracle network, or multi-sig.
- **Real ECDSA verification**: The relayer signs proof bundles with a real ECDSA key; verification uses `ecrecover` to check the signer matches the expected relayer address. Merkle inclusion proofs are deterministic but structurally simulated.

## Simulated Bridge Explanation

Real cross-chain bridges lock assets on one chain and mint representative tokens on another. In this demo:
- Funds are locked in the Ethereum escrow contract
- Solana execution is simulated — the deterministic computation (`amount × 2`) runs locally in the relayer. See `reference/solana-program/` for the full on-chain Solana program implementation.
- Settlement on Ethereum requires the relayer to present a signed proof bundle
- No real tokens cross any chain boundary

## Verification Explanation

The relayer uses real ECDSA cryptography for proof bundles:
- Block header and event root are SHA-256 hashes of real data
- Merkle inclusion proof nodes are deterministic (seeded by nonce), not random
- The validator signature is a **real ECDSA signature** over `keccak256(block_header || event_root || nonce)`
- Verification uses `ecrecover` to check the signer matches the relayer's address
- The Ethereum `settle()` function independently verifies the relayer's signature on-chain

This is the "Validator Signature" approach used by production bridges like early Wormhole and Ronin.

## Message Lifecycle

1. User calls `lockFunds()` on Ethereum → funds escrowed, `CrossChainRequest` event emitted
2. Relayer observes the event → persists to SQLite → status: `Observed → Persisted`
3. Relayer generates ECDSA-signed proof bundle → status: `Verified`
4. Relayer simulates Solana execution (deterministic: `amount × 2`) → status: `Executed`
5. Relayer constructs settlement with signature → calls `settle()` on Ethereum → status: `Settled`
6. Funds released (or refunded on timeout)

On transient failure, messages retry once. If the retry also fails, the message is **rolled back** and the escrow is refunded.

## Event Model

All components emit events with this structure:
```json
{
  "trace_id": "uuid",
  "nonce": 1,
  "actor": "ethereum | relayer | solana | dashboard",
  "step": "locked | observed | verified | executed | minted | burned | rollback | settled",
  "status": "success | failure | retry",
  "timestamp": "iso8601"
}
```

## How to Run Locally

### Prerequisites
- [Foundry](https://book.getfoundry.sh/) (for Anvil + Solidity)
- [Rust + Cargo](https://rustup.rs/)
- [Node.js 18+](https://nodejs.org/)
- [Docker + Docker Compose](https://docs.docker.com/compose/) (optional)

### Quick Start

```bash
# 1. Start Anvil (local Ethereum node)
anvil &

# 2. Deploy Ethereum contract
cd eth-contract && forge script script/Deploy.s.sol --broadcast --rpc-url http://127.0.0.1:8545

# 3. Start relayer (includes embedded traffic generator)
cd relayer && cargo run

# 4. Start dashboard
cd dashboard-web && npm install && npm run dev
```

The relayer includes an embedded traffic generator — click **Start** in the dashboard header to begin the simulation.

### Docker Compose
```bash
docker-compose up
```

## Limitations

- Single trusted relayer (no fault tolerance or decentralization)
- Simulated bridge (no real token transfers across chains)
- Solana execution is simulated locally (see `reference/solana-program/` for the real program)
- Local-only (Anvil)
- No MEV protection or ordering guarantees
- Settlement signatures use a single ECDSA key, not a threshold signature from a validator set
