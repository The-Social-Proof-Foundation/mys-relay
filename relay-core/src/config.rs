use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub redpanda: RedpandaConfig,
    pub server: ServerConfig,
    pub delivery: DeliveryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedpandaConfig {
    pub brokers: String,
    pub consumer_group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub api_port: u16,
    pub ws_port: u16,
    pub host: String,
    pub jwt_secret: String,
    pub encryption_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryConfig {
    pub apns_bundle_id: Option<String>,
    pub apns_key_id: Option<String>,
    pub apns_team_id: Option<String>,
    pub apns_key_path: Option<String>,
    pub apns_key_content: Option<String>, // Base64 encoded key content (alternative to path)
    pub fcm_server_key: Option<String>,
    pub resend_api_key: Option<String>,
    pub resend_from_email: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let _ = dotenv::dotenv();

        Config {
            database: DatabaseConfig {
                url: env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/mys_social_indexer".to_string()),
                max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()
                    .unwrap_or(10),
            },
            redis: RedisConfig {
                url: env::var("REDIS_URL")
                    .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
                max_connections: env::var("REDIS_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()
                    .unwrap_or(10),
            },
            redpanda: RedpandaConfig {
                brokers: env::var("REDPANDA_BROKERS")
                    .unwrap_or_else(|_| "localhost:9092".to_string()),
                consumer_group: env::var("REDPANDA_CONSUMER_GROUP")
                    .unwrap_or_else(|_| "relay-consumer-group".to_string()),
            },
            server: ServerConfig {
                host: env::var("SERVER_HOST")
                    .unwrap_or_else(|_| "0.0.0.0".to_string()),
                api_port: env::var("API_PORT")
                    .or_else(|_| env::var("PORT"))
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()
                    .unwrap_or(8080),
                ws_port: env::var("WS_PORT")
                    .unwrap_or_else(|_| "8081".to_string())
                    .parse()
                    .unwrap_or(8081),
                jwt_secret: env::var("JWT_SECRET")
                    .unwrap_or_else(|_| "your-secret-key-change-in-production".to_string()),
                encryption_key: env::var("ENCRYPTION_KEY")
                    .unwrap_or_else(|_| {
                        // Generate a default key for development (32 bytes base64)
                        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
                    }),
            },
            delivery: DeliveryConfig {
                apns_bundle_id: env::var("APNS_BUNDLE_ID").ok(),
                apns_key_id: env::var("APNS_KEY_ID").ok(),
                apns_team_id: env::var("APNS_TEAM_ID").ok(),
                apns_key_path: env::var("APNS_KEY_PATH").ok(),
                apns_key_content: env::var("APNS_KEY_CONTENT").ok(),
                fcm_server_key: env::var("FCM_SERVER_KEY").ok(),
                resend_api_key: env::var("RESEND_API_KEY").ok(),
                resend_from_email: env::var("RESEND_FROM_EMAIL").ok(),
            },
        }
    }
}

