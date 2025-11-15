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

fn build_client_config(config: &RedpandaConfig) -> ClientConfig {
    let mut client_config = ClientConfig::new();
    
    client_config
        .set("bootstrap.servers", &config.brokers)
        .set("metadata.request.timeout.ms", "30000")
        .set("socket.timeout.ms", "30000")
        .set("socket.keepalive.enable", "true")
        // Force IPv4 to avoid IPv6 connection issues
        // Railway internal networking may resolve to IPv6 but service only listens on IPv4
        .set("broker.address.family", "v4");
    
    tracing::info!("Configured to prefer IPv4 connections (broker.address.family=v4)");
    
    // Add SSL/TLS configuration if REDPANDA_SSL_ENABLED is set
    if let Ok(ssl_enabled) = std::env::var("REDPANDA_SSL_ENABLED") {
        if ssl_enabled == "true" || ssl_enabled == "1" {
            tracing::info!("SSL/TLS enabled for Redpanda connection");
            client_config
                .set("security.protocol", "ssl");
            
            // Optional: SSL certificate and key paths
            if let Ok(ca_location) = std::env::var("REDPANDA_SSL_CA_LOCATION") {
                client_config.set("ssl.ca.location", &ca_location);
            }
            if let Ok(cert_location) = std::env::var("REDPANDA_SSL_CERT_LOCATION") {
                client_config.set("ssl.certificate.location", &cert_location);
            }
            if let Ok(key_location) = std::env::var("REDPANDA_SSL_KEY_LOCATION") {
                client_config.set("ssl.key.location", &key_location);
            }
        }
    }
    
    client_config
}

pub fn create_producer(config: &RedpandaConfig) -> Result<RedpandaProducer> {
    tracing::info!("Creating Redpanda producer");
    tracing::info!("Brokers: {}", config.brokers);
    
    if config.brokers.contains(".railway.app") {
        tracing::warn!("Using Railway public URL for brokers. Consider using internal Railway networking (.railway.internal) for better connectivity.");
    }

    let producer: FutureProducer = build_client_config(config)
        .set("message.timeout.ms", "5000")
        .set("acks", "all")
        .set("retries", "3")
        .create()
        .map_err(|e| {
            tracing::error!("Failed to create Redpanda producer: {}", e);
            tracing::error!("Broker address: {}", config.brokers);
            tracing::error!("Common issues:");
            tracing::error!("  1. Broker not accessible at this address");
            tracing::error!("  2. Network/firewall blocking connection");
            tracing::error!("  3. SSL/TLS required but not configured (set REDPANDA_SSL_ENABLED=true)");
            tracing::error!("  4. For Railway: use internal networking (.railway.internal) instead of public URL");
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
    
    if config.brokers.contains(".railway.app") {
        tracing::warn!("Using Railway public URL for brokers. Consider using internal Railway networking (.railway.internal) for better connectivity.");
    }

    let consumer: StreamConsumer = build_client_config(config)
        .set("group.id", group)
        .set("enable.partition.eof", "false")
        .set("session.timeout.ms", "30000")
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .create()
        .map_err(|e| {
            tracing::error!("Failed to create Redpanda consumer: {}", e);
            tracing::error!("Broker address: {}", config.brokers);
            tracing::error!("Consumer group: {}", group);
            tracing::error!("Common issues:");
            tracing::error!("  1. Broker not accessible at this address");
            tracing::error!("  2. Network/firewall blocking connection");
            tracing::error!("  3. SSL/TLS required but not configured (set REDPANDA_SSL_ENABLED=true)");
            tracing::error!("  4. For Railway: use internal networking (.railway.internal) instead of public URL");
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

