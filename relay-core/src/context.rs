use std::sync::Arc;
use crate::config::Config;
use crate::db::{DbPool, create_pool as create_db_pool};
use crate::redis::{RedisPool, create_pool as create_redis_pool};
use crate::redpanda::{RedpandaProducer, RedpandaConsumer, create_producer, create_consumer};

#[derive(Clone)]
pub struct RelayContext {
    pub config: Arc<Config>,
    pub db_pool: Arc<DbPool>,
    pub redis_pool: RedisPool,
    pub redpanda_producer: RedpandaProducer,
}

impl RelayContext {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let db_pool = create_db_pool(&config.database).await?;
        let redis_pool = create_redis_pool(&config.redis).await?;
        let redpanda_producer = create_producer(&config.redpanda)?;

        Ok(RelayContext {
            config: Arc::new(config),
            db_pool,
            redis_pool,
            redpanda_producer,
        })
    }

    pub fn create_consumer(&self, group_id: Option<&str>) -> anyhow::Result<RedpandaConsumer> {
        create_consumer(&self.config.redpanda, group_id)
    }
}

