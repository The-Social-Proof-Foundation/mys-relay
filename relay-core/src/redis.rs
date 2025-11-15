use anyhow::{anyhow, Result};
use redis::aio::MultiplexedConnection;
use redis::Client;
use std::sync::Arc;
use tracing;

use crate::config::RedisConfig;

pub type RedisPool = Arc<Client>;
pub type RedisConnection = MultiplexedConnection;

pub async fn create_pool(config: &RedisConfig) -> Result<RedisPool> {
    tracing::info!("Setting up Redis connection pool");
    tracing::info!("Redis URL: {}", mask_redis_url(&config.url));

    let client = Client::open(config.url.as_str())
        .map_err(|e| anyhow!("Failed to create Redis client: {}", e))?;

    // Test the connection
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| anyhow!("Failed to connect to Redis: {}", e))?;

    redis::cmd("PING")
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| anyhow!("Failed to ping Redis: {}", e))?;

    tracing::info!("Redis connection established successfully!");

    Ok(Arc::new(client))
}

pub async fn get_connection(pool: &RedisPool) -> Result<RedisConnection> {
    pool.get_multiplexed_async_connection()
        .await
        .map_err(|e| anyhow!("Failed to get Redis connection: {}", e))
}

fn mask_redis_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        let (before_at, after_at) = url.split_at(at_pos);
        if let Some(colon_pos) = before_at.rfind(':') {
            let (protocol_user, _password) = before_at.split_at(colon_pos);
            format!("{}:****@{}", protocol_user, after_at)
        } else {
            format!("redis://****@{}", after_at)
        }
    } else {
        url.to_string()
    }
}

