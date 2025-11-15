use anyhow::Result;
use relay_core::Config;
use relay_core::RelayContext;
use relay_outbox::run as run_outbox;
use relay_notify::run as run_notify;
use relay_messaging::run as run_messaging;
use relay_delivery::run as run_delivery;
use relay_api::run as run_api;
use tokio;
use tracing;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting MySocial Relay Server");

    // Load configuration
    let config = Config::from_env();
    let ctx = RelayContext::new(config).await?;

    tracing::info!("Relay context initialized");

    // Spawn all modules as parallel tasks
    let ctx_clone = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_outbox(ctx_clone).await {
            tracing::error!("Outbox poller error: {}", e);
        }
    });

    let ctx_clone = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_notify(ctx_clone).await {
            tracing::error!("Notification consumer error: {}", e);
        }
    });

    let ctx_clone = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_messaging(ctx_clone).await {
            tracing::error!("Messaging consumer error: {}", e);
        }
    });

    let ctx_clone = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_delivery(ctx_clone).await {
            tracing::error!("Delivery consumer error: {}", e);
        }
    });

    // API server runs in main task
    tracing::info!("Starting API server");
    run_api(ctx).await?;

    Ok(())
}

