use anyhow::{anyhow, Result};
use diesel_async::pooled_connection::deadpool::{Object, Pool};
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use std::sync::Arc;
use tokio::time::Duration;
use tracing;

use crate::config::DatabaseConfig;

pub type DbPool = Pool<AsyncPgConnection>;
pub type DbConnection = Object<AsyncPgConnection>;

pub async fn create_pool(config: &DatabaseConfig) -> Result<Arc<DbPool>> {
    tracing::info!("Setting up database connection pool");
    tracing::info!("Database URL: {}", mask_database_url(&config.url));

    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(&config.url);

    let pool = Pool::builder(manager)
        .max_size(config.max_connections as usize)
        .build()
        .map_err(|e| anyhow!("Failed to create connection pool: {}", e))?;

    tracing::info!("Database connection pool created, testing connection...");

    // Test the connection with retry logic
    let mut last_error = None;
    for attempt in 1..=5 {
        tracing::info!("Connection attempt {} of 5", attempt);

        match tokio::time::timeout(Duration::from_secs(15), pool.get()).await {
            Ok(Ok(_conn)) => {
                tracing::info!("Database connection established successfully!");
                return Ok(Arc::new(pool));
            }
            Ok(Err(e)) => {
                tracing::warn!("Database connection failed on attempt {}: {}", attempt, e);
                last_error = Some(anyhow!("Database connection failed: {}", e));
            }
            Err(_) => {
                tracing::warn!("Database connection timed out on attempt {}", attempt);
                last_error = Some(anyhow!("Database connection timed out"));
            }
        }

        if attempt < 5 {
            let wait_time = Duration::from_secs(2_u64.pow(attempt - 1));
            tracing::info!("Waiting {:?} before retry...", wait_time);
            tokio::time::sleep(wait_time).await;
        }
    }

    tracing::error!("All database connection attempts failed");
    if let Some(err) = last_error {
        return Err(err);
    }

    Err(anyhow!("Failed to establish database connection after 5 attempts"))
}

fn mask_database_url(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        let (before_at, after_at) = url.split_at(at_pos);
        if let Some(colon_pos) = before_at.rfind(':') {
            let (protocol_user, _password) = before_at.split_at(colon_pos);
            format!("{}:****@{}", protocol_user, after_at)
        } else {
            "postgres://****@****".to_string()
        }
    } else {
        "Invalid URL format".to_string()
    }
}

