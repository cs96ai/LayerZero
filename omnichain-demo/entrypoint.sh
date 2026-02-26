#!/bin/bash
set -e

echo "=== Omnichain Escrow Demo - Starting ==="
echo "Dashboard: http://localhost:3001"
echo ""

# Anvil default account #0 (deployer + relayer)
DEPLOYER_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
RELAYER_ADDR="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
RPC="http://127.0.0.1:8545"

# ── 1. Start Anvil ──
echo "[1/3] Starting Anvil (local Ethereum node)..."
anvil --host 0.0.0.0 --port 8545 --silent &
ANVIL_PID=$!

echo "Waiting for Anvil..."
for i in $(seq 1 30); do
    if curl -sf "$RPC" -X POST -H "Content-Type: application/json" \
       -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' > /dev/null 2>&1; then
        echo "Anvil is ready (PID $ANVIL_PID)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "ERROR: Anvil failed to start after 30s"
        exit 1
    fi
    sleep 1
done

# ── 2. Deploy the escrow contract ──
echo "[2/3] Deploying CrossChainEscrow contract..."

# Extract bytecode from the compiled artifact
BYTECODE=$(jq -r '.bytecode.object' /contract/CrossChainEscrow.json)

# Constructor args: address _relayer, uint256 _timeout (3600 = 1 hour)
# ABI-encode: address is padded to 32 bytes, uint256 is 3600 = 0xe10
CONSTRUCTOR_ARGS=$(cast abi-encode "constructor(address,uint256)" "$RELAYER_ADDR" 3600)

# Deploy using cast
DEPLOYED=$(cast send --private-key "$DEPLOYER_KEY" \
    --rpc-url "$RPC" \
    --create "${BYTECODE}${CONSTRUCTOR_ARGS:2}" \
    --json 2>/dev/null | jq -r '.contractAddress')

if [ -z "$DEPLOYED" ] || [ "$DEPLOYED" = "null" ]; then
    echo "WARNING: Contract deployment may have failed. Using default address."
    DEPLOYED="0x5FbDB2315678afecb367f032d93F642f64180aa3"
fi

echo "Contract deployed at: $DEPLOYED"
export ESCROW_ADDRESS="$DEPLOYED"

# ── 3. Start the relayer ──
echo "[3/3] Starting relayer (API + dashboard on port 3001)..."
exec relayer
