use anyhow::{Result, anyhow};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use relay_core::schema::relay_outbox;
use relay_core::{RelayContext, redpanda::produce_message};
use std::time::Duration;
use tracing;

#[derive(Queryable, Selectable)]
#[diesel(table_name = relay_core::schema::relay_outbox)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct OutboxRow {
    id: i64,
    event_type: String,
    event_data: serde_json::Value,
    event_id: Option<String>,
    transaction_id: Option<String>,
}

const POLL_INTERVAL_MS: u64 = 150;
const BATCH_SIZE: usize = 100;
const MAX_RETRIES: i32 = 3;

pub async fn run(ctx: RelayContext) -> Result<()> {
    tracing::info!("Starting outbox poller");

    loop {
        match poll_and_publish(&ctx).await {
            Ok(_) => {
                tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
            }
            Err(e) => {
                tracing::error!("Error in outbox poller: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn poll_and_publish(ctx: &RelayContext) -> Result<()> {
    let mut conn = ctx.db_pool.get().await?;

    // Query unprocessed events
    let events: Vec<OutboxRow> = 
        relay_outbox::table
            .filter(relay_outbox::processed_at.is_null())
            .filter(relay_outbox::retry_count.lt(&MAX_RETRIES))
            .order(relay_outbox::created_at.asc())
            .limit(BATCH_SIZE as i64)
            .select(OutboxRow::as_select())
            .load(&mut conn)
            .await?;

    if events.is_empty() {
        return Ok(());
    }

    tracing::debug!("Found {} unprocessed events", events.len());

    for event in events {
        match publish_event(ctx, &event.event_type, &event.event_data, event.event_id.as_deref(), event.transaction_id.as_deref()).await {
            Ok(_) => {
                // Mark as processed
                diesel::update(relay_outbox::table.filter(relay_outbox::id.eq(event.id)))
                    .set((
                        relay_outbox::processed_at.eq(Utc::now()),
                        relay_outbox::published_at.eq(Utc::now()),
                    ))
                    .execute(&mut conn)
                    .await?;

                tracing::debug!("Published and marked event {} as processed", event.id);
            }
            Err(e) => {
                // Increment retry count
                diesel::update(relay_outbox::table.filter(relay_outbox::id.eq(event.id)))
                    .set((
                        relay_outbox::retry_count.eq(relay_outbox::retry_count + 1),
                        relay_outbox::error_message.eq(Some(format!("{}", e))),
                    ))
                    .execute(&mut conn)
                    .await?;

                tracing::warn!("Failed to publish event {}: {}", event.id, e);
            }
        }
    }

    Ok(())
}

async fn publish_event(
    ctx: &RelayContext,
    event_type: &str,
    event_data: &serde_json::Value,
    event_id: Option<&str>,
    transaction_id: Option<&str>,
) -> Result<()> {
    // Determine topic from event type
    let topic = match event_type {
        t if t.starts_with("like.") => "events.like.created",
        t if t.starts_with("comment.") => "events.comment.created",
        t if t.starts_with("message.") => "events.message.created",
        t if t.starts_with("follow.") => "events.follow.created",
        t if t.starts_with("unfollow.") => "events.unfollow.created",
        _ => "events.unknown",
    };

    // Create message payload
    let payload = serde_json::json!({
        "event_type": event_type,
        "event_data": event_data,
        "event_id": event_id,
        "transaction_id": transaction_id,
        "timestamp": Utc::now(),
    });

    let payload_bytes = serde_json::to_vec(&payload)?;

    // Use event_id as key if available, otherwise use transaction_id
    let key = event_id.or(transaction_id);

    produce_message(&ctx.redpanda_producer, topic, key, &payload_bytes).await?;

    tracing::debug!("Published event {} to topic {}", event_type, topic);

    Ok(())
}

