use std::env;

#[derive(Clone, Debug)]
pub enum LogFormat {
    Json,
    Pretty,
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(LogFormat::Json),
            "pretty" => Ok(LogFormat::Pretty),
            other => Err(format!(
                "unknown log format: {other}, expected 'json' or 'pretty'"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    /// gRPC endpoint for Sui fullnode (e.g., "https://fullnode.testnet.sui.io:443")
    pub sui_grpc_url: String,
    /// GraphQL endpoint for indexed queries (historical object/event scans)
    pub sui_graphql_url: String,
    /// Sentinel package ID on chain
    pub sentinel_package_id: String,
    /// ThreatRegistry shared object ID
    pub threat_registry_id: String,
    /// Admin private key (ed25519, bech32 suiprivkey1... format)
    pub publisher_private_key: String,
    /// World package ID (for event type filtering)
    pub world_package_id: String,
    /// Bounty board package ID (for bounty event filtering)
    pub bounty_board_package_id: String,
    /// How often to publish scores on-chain (ms)
    pub publish_interval_ms: u64,
    /// Minimum score change (basis points) to trigger an on-chain publish
    pub publish_score_threshold_bp: u64,
    /// HTTP API port
    pub api_port: u16,
    /// Postgres connection string
    pub database_url: String,
    /// World REST API base URL (Stillness)
    pub world_api_url: String,
    /// Log level for sentinel_backend crate
    pub sentinel_log_level: tracing::Level,
    /// Log level for all other crates (tonic, hyper, etc.)
    pub crates_log_level: tracing::Level,
    /// Log output format
    pub log_format: LogFormat,
    /// Discord bot token (requires "discord" feature + DISCORD_TOKEN env var)
    #[cfg(feature = "discord")]
    pub discord_token: String,
}

fn require(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

impl AppConfig {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            sui_grpc_url: require("SUI_GRPC_URL"),
            sui_graphql_url: require("SUI_GRAPHQL_URL"),
            sentinel_package_id: require("SENTINEL_PACKAGE_ID"),
            threat_registry_id: require("THREAT_REGISTRY_ID"),
            publisher_private_key: require("SUI_PUBLISHER_KEY"),
            world_package_id: require("WORLD_PACKAGE_ID"),
            bounty_board_package_id: require("BUILDER_PACKAGE_ID"),
            publish_interval_ms: require("SENTINEL_PUBLISH_INTERVAL_MS")
                .parse()
                .expect("SENTINEL_PUBLISH_INTERVAL_MS must be a number"),
            publish_score_threshold_bp: require("SENTINEL_PUBLISH_THRESHOLD_BP")
                .parse()
                .expect("SENTINEL_PUBLISH_THRESHOLD_BP must be a number"),
            api_port: require("SENTINEL_API_PORT")
                .parse()
                .expect("SENTINEL_API_PORT must be a number"),
            database_url: require("DATABASE_URL"),
            world_api_url: require("WORLD_API_URL"),
            sentinel_log_level: require("SENTINEL_LOG_LEVEL")
                .parse()
                .expect("SENTINEL_LOG_LEVEL must be one of: error, warn, info, debug, trace"),
            crates_log_level: require("CRATES_LOG_LEVEL")
                .parse()
                .expect("CRATES_LOG_LEVEL must be one of: error, warn, info, debug, trace"),
            log_format: require("LOG_FORMAT")
                .parse()
                .expect("LOG_FORMAT must be 'json' or 'pretty'"),
            #[cfg(feature = "discord")]
            discord_token: require("DISCORD_TOKEN"),
        })
    }
}
