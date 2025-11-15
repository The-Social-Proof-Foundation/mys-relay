use anyhow::Result;
use relay_core::Config;
use relay_core::RelayContext;
use relay_outbox::run as run_outbox;
use relay_notify::run as run_notify;
use relay_messaging::run as run_messaging;
use relay_delivery::run as run_delivery;
use relay_api::run as run_api;
use tokio;
use tokio::signal;
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

    // Validate production secrets
    validate_production_secrets();

    // Load configuration
    let config = Config::from_env();
    let ctx = RelayContext::new(config).await?;

    tracing::info!("Relay context initialized");

    // Create shutdown signal
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    // Spawn signal handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                tracing::info!("Received Ctrl+C, initiating graceful shutdown...");
            },
            _ = terminate => {
                tracing::info!("Received SIGTERM, initiating graceful shutdown...");
            },
        }

        let _ = shutdown_tx_clone.send(());
    });

    // Spawn all modules as parallel tasks with shutdown handling
    let ctx_clone = ctx.clone();
    let mut shutdown_rx_clone = shutdown_rx.resubscribe();
    tokio::spawn(async move {
        tokio::select! {
            result = run_outbox(ctx_clone) => {
                if let Err(e) = result {
                    tracing::error!("Outbox poller error: {}", e);
                }
            },
            _ = shutdown_rx_clone.recv() => {
                tracing::info!("Outbox poller shutting down...");
            },
        }
    });

    let ctx_clone = ctx.clone();
    let mut shutdown_rx_clone = shutdown_rx.resubscribe();
    tokio::spawn(async move {
        tokio::select! {
            result = run_notify(ctx_clone) => {
                if let Err(e) = result {
                    tracing::error!("Notification consumer error: {}", e);
                }
            },
            _ = shutdown_rx_clone.recv() => {
                tracing::info!("Notification consumer shutting down...");
            },
        }
    });

    let ctx_clone = ctx.clone();
    let mut shutdown_rx_clone = shutdown_rx.resubscribe();
    tokio::spawn(async move {
        tokio::select! {
            result = run_messaging(ctx_clone) => {
                if let Err(e) = result {
                    tracing::error!("Messaging consumer error: {}", e);
                }
            },
            _ = shutdown_rx_clone.recv() => {
                tracing::info!("Messaging consumer shutting down...");
            },
        }
    });

    let ctx_clone = ctx.clone();
    let mut shutdown_rx_clone = shutdown_rx.resubscribe();
    tokio::spawn(async move {
        tokio::select! {
            result = run_delivery(ctx_clone) => {
                if let Err(e) = result {
                    tracing::error!("Delivery consumer error: {}", e);
                }
            },
            _ = shutdown_rx_clone.recv() => {
                tracing::info!("Delivery consumer shutting down...");
            },
        }
    });

    // API server runs in main task with shutdown handling
    tracing::info!("Starting API server");
    
    tokio::select! {
        result = run_api(ctx) => {
            if let Err(e) = result {
                tracing::error!("API server error: {}", e);
            }
        },
        _ = shutdown_rx.recv() => {
            tracing::info!("API server shutting down...");
        },
    }

    tracing::info!("MySocial Relay Server shutdown complete");
    Ok(())
}

fn validate_production_secrets() {
    use std::env;
    
    let jwt_secret = env::var("JWT_SECRET").unwrap_or_default();
    let encryption_key = env::var("ENCRYPTION_KEY").unwrap_or_default();
    
    // Check if running in production (Railway sets RAILWAY_ENVIRONMENT)
    let is_production = env::var("RAILWAY_ENVIRONMENT").is_ok() 
        || env::var("RAILWAY_SERVICE_NAME").is_ok()
        || env::var("PRODUCTION").is_ok();
    
    if is_production {
        if jwt_secret.is_empty() || jwt_secret == "your-secret-key-change-in-production" {
            tracing::error!("JWT_SECRET is not set or using default value in production!");
            tracing::error!("This is a security risk. Please set JWT_SECRET environment variable.");
            // Don't panic, but log strongly
        }
        
        if encryption_key.is_empty() || encryption_key == "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" {
            tracing::error!("ENCRYPTION_KEY is not set or using default value in production!");
            tracing::error!("This is a security risk. Please set ENCRYPTION_KEY environment variable.");
            // Don't panic, but log strongly
        }
    }
}
