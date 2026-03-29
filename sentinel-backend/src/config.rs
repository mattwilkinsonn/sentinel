use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// gRPC endpoint for Sui fullnode (e.g., "https://fullnode.testnet.sui.io:443")
    pub sui_grpc_url: String,
    /// Sentinel package ID on chain
    pub sentinel_package_id: String,
    /// ThreatRegistry shared object ID
    pub threat_registry_id: String,
    /// Admin private key (ed25519, hex or base64)
    pub admin_private_key: String,
    /// World package ID (for event type filtering)
    pub world_package_id: String,
    /// Bounty board package ID (for bounty event filtering)
    pub bounty_board_package_id: String,
    /// How often to publish scores on-chain (ms)
    pub publish_interval_ms: u64,
    /// HTTP API port
    pub api_port: u16,
    /// Postgres connection string
    pub database_url: String,
    /// World REST API base URL (Stillness)
    pub world_api_url: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            sui_grpc_url: env::var("SUI_GRPC_URL")
                .unwrap_or_else(|_| "https://fullnode.testnet.sui.io:443".into()),
            sentinel_package_id: env::var("SENTINEL_PACKAGE_ID").unwrap_or_default(),
            threat_registry_id: env::var("THREAT_REGISTRY_ID").unwrap_or_default(),
            admin_private_key: env::var("ADMIN_PRIVATE_KEY").unwrap_or_default(),
            world_package_id: env::var("WORLD_PACKAGE_ID").unwrap_or_default(),
            bounty_board_package_id: env::var("BUILDER_PACKAGE_ID").unwrap_or_default(),
            publish_interval_ms: env::var("SENTINEL_PUBLISH_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30_000),
            api_port: env::var("SENTINEL_API_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3001),
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL is required"),
            world_api_url: env::var("WORLD_API_URL")
                .unwrap_or_else(|_| "https://world-api-stillness.live.tech.evefrontier.com".into()),
        })
    }
}
