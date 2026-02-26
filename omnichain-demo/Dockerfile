# ============================================================
# Unified Dockerfile: Anvil + Relayer + Dashboard (static)
# Single container, single port (3001), for Azure Container Apps
# ============================================================

# ── Stage 1: Build the React dashboard ──
FROM node:18-slim AS dashboard-builder
WORKDIR /app
COPY dashboard-web/package.json dashboard-web/package-lock.json* ./
RUN npm install
COPY dashboard-web/ .
RUN npm run build

# ── Stage 2: Build the Rust relayer ──
FROM rust:1-slim AS relayer-builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY relayer/Cargo.toml relayer/Cargo.lock* ./
COPY relayer/src/ src/
# Strip the nightly-only cargo-features and dev codegen-backend before building
RUN sed -i '/^cargo-features/d' Cargo.toml && \
    sed -i '/codegen-backend/d' Cargo.toml && \
    rm -f Cargo.lock && \
    cargo build --release

# ── Stage 3: Compile the Solidity contract ──
FROM ghcr.io/foundry-rs/foundry:latest AS contract-builder
WORKDIR /tmp/build
COPY --chown=foundry:foundry eth-contract/ .
RUN forge build

# ── Stage 4: Final runtime image ──
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 curl jq \
    && rm -rf /var/lib/apt/lists/*

# Copy anvil + cast binaries directly from the foundry image (no foundryup needed)
COPY --from=ghcr.io/foundry-rs/foundry:latest /usr/local/bin/anvil /usr/local/bin/anvil
COPY --from=ghcr.io/foundry-rs/foundry:latest /usr/local/bin/cast /usr/local/bin/cast

# Copy relayer binary
COPY --from=relayer-builder /app/target/release/relayer /usr/local/bin/relayer

# Copy compiled contract artifact (for deployment via cast)
COPY --from=contract-builder /tmp/build/out/CrossChainEscrow.sol/CrossChainEscrow.json /contract/CrossChainEscrow.json

# Copy dashboard static build
COPY --from=dashboard-builder /app/dist /dashboard

# Copy the entrypoint script
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Environment defaults
ENV ETH_RPC_URL=http://127.0.0.1:8545 \
    DATABASE_URL=sqlite:relayer.db?mode=rwc \
    RELAYER_HTTP_PORT=3001 \
    AUTO_START_SIMULATION=true \
    RUST_LOG=relayer=info

EXPOSE 3001

ENTRYPOINT ["/entrypoint.sh"]
