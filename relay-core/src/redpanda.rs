use anyhow::{anyhow, Result};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::StreamConsumer;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::sync::Arc;
use std::time::Duration;
use tracing;

use crate::config::RedpandaConfig;

pub type RedpandaProducer = Arc<FutureProducer>;
pub type RedpandaConsumer = Arc<StreamConsumer>;

pub fn create_producer(config: &RedpandaConfig) -> Result<RedpandaProducer> {
    tracing::info!("Creating Redpanda producer");
    tracing::info!("Brokers: {}", config.brokers);

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &config.brokers)
        .set("message.timeout.ms", "5000")
        .set("acks", "all")
        .set("retries", "3")
        .set("metadata.request.timeout.ms", "10000")
        .set("socket.timeout.ms", "10000")
        .create()
        .map_err(|e| {
            tracing::error!("Failed to create Redpanda producer: {}", e);
            tracing::error!("Please verify REDPANDA_BROKERS is set correctly and brokers are accessible");
            anyhow!("Failed to create Redpanda producer: {}", e)
        })?;

    tracing::info!("Redpanda producer created successfully (connection will be established on first use)");

    Ok(Arc::new(producer))
}

pub fn create_consumer(config: &RedpandaConfig, group_id: Option<&str>) -> Result<RedpandaConsumer> {
    let group = group_id.unwrap_or(&config.consumer_group);
    tracing::info!("Creating Redpanda consumer");
    tracing::info!("Brokers: {}", config.brokers);
    tracing::info!("Consumer group: {}", group);

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &config.brokers)
        .set("group.id", group)
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "6000")
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("metadata.request.timeout.ms", "10000")
        .set("socket.timeout.ms", "10000")
        .create()
        .map_err(|e| {
            tracing::error!("Failed to create Redpanda consumer: {}", e);
            tracing::error!("Please verify REDPANDA_BROKERS is set correctly and brokers are accessible");
            anyhow!("Failed to create Redpanda consumer: {}", e)
        })?;

    tracing::info!("Redpanda consumer created successfully (connection will be established on first use)");

    Ok(Arc::new(consumer))
}

pub async fn produce_message(
    producer: &RedpandaProducer,
    topic: &str,
    key: Option<&str>,
    payload: &[u8],
) -> Result<()> {
    let mut record = FutureRecord::to(topic).payload(payload);

    if let Some(k) = key {
        record = record.key(k);
    }

    match producer.send(record, Duration::from_secs(5)).await {
        Ok((partition, offset)) => {
            tracing::debug!(
                "Message delivered to topic {} partition {} offset {}",
                topic,
                partition,
                offset
            );
            Ok(())
        }
        Err((e, _)) => {
            tracing::error!("Failed to deliver message to topic {}: {:?}", topic, e);
            Err(anyhow!("Failed to deliver message: {:?}", e))
        }
    }
}

