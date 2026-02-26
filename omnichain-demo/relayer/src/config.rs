use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub eth_rpc_url: String,
    pub database_url: String,
    pub http_port: u16,
    pub escrow_address: String,
    pub relayer_private_key: String,
    pub poll_interval_ms: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            eth_rpc_url: env::var("ETH_RPC_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8545".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:relayer.db?mode=rwc".into()),
            http_port: env::var("RELAYER_HTTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3001),
            escrow_address: env::var("ESCROW_ADDRESS")
                .unwrap_or_else(|_| "0x5FbDB2315678afecb367f032d93F642f64180aa3".into()),
            // Anvil default account #0 private key
            relayer_private_key: env::var("RELAYER_PRIVATE_KEY").unwrap_or_else(|_| {
                "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".into()
            }),
            poll_interval_ms: env::var("POLL_INTERVAL_MS")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(500),
        }
    }
}
