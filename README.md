# MySocial Relay Server

A production-ready notification and messaging relay server for the MySocial platform. The relay server handles real-time notifications, messaging, push notifications (APNs/FCM), email delivery, and WebSocket connections.

## Architecture

The relay server follows a modular, single-binary architecture where all services run as parallel Tokio tasks within one `relay-runner` binary:

```
relay-runner (main binary)
├── relay-core (shared config, DB pool, Redis pool, types, context)
├── relay-api (REST API server + WebSocket server)
├── relay-notify (notification processing service)
├── relay-messaging (messaging service)
├── relay-delivery (APNs/FCM/Email delivery workers)
└── relay-outbox (CDC poller for Postgres → Redpanda)
```

### Key Components

- **Outbox Poller**: Polls `relay_outbox` table (written by indexer) and publishes events to Redpanda
- **Notification Service**: Consumes notification events from Redpanda, stores in Postgres/Redis, and triggers delivery
- **Messaging Service**: Handles direct messages between users
- **Delivery Service**: Sends push notifications (APNs/FCM) and emails (Resend) to users
- **API Server**: REST API for notifications, messages, preferences, and WebSocket for real-time updates

## Features

### Notifications
- ✅ Real-time notification processing from blockchain events
- ✅ Platform-specific notification filtering
- ✅ Per-user and per-platform unread notification counts
- ✅ Redis-backed inbox for fast retrieval
- ✅ Postgres persistence for historical data
- ✅ WebSocket support for real-time updates

### Messaging
- ✅ Direct messaging between users
- ✅ Conversation tracking
- ✅ Redis Streams for real-time message delivery
- ✅ Message read receipts

### Delivery
- ✅ **Platform-specific delivery configuration**: Each platform can configure its own APNs, FCM, and email settings
- ✅ **APNs (iOS)**: Token-based authentication with support for key file or base64-encoded key content
- ✅ **FCM (Android)**: Firebase Cloud Messaging integration
- ✅ **Email (Resend)**: Direct API integration for email delivery
- ✅ Fallback to global delivery config when platform config is missing

### Platform Configuration

The relay server supports platform-specific delivery configuration stored in the `platform_delivery_config` table:

- **APNs**: `apns_bundle_id`, `apns_key_id`, `apns_team_id`, `apns_key_path` or `apns_key_content` (base64)
- **FCM**: `fcm_server_key`
- **Resend**: `resend_api_key`, `resend_from_email`

When a notification includes a `platform_id`, the relay server:
1. Looks up platform-specific delivery configuration
2. Creates platform-specific delivery clients
3. Falls back to global config if platform config is missing

## Database Schema

### Core Tables

- `relay_outbox`: CDC table written by indexer, polled by relay
- `relay_notifications`: User notifications with platform_id support
- `relay_messages`: Direct messages between users
- `relay_conversations`: Conversation metadata
- `relay_user_preferences`: User notification preferences
- `relay_device_tokens`: Device tokens for push notifications
- `relay_ws_connections`: Active WebSocket connections
- `platform_delivery_config`: Platform-specific delivery settings

## Redis Keys

- `INBOX:{user_address}`: List of recent notifications (last 100)
- `UNREAD:{user_address}`: Total unread notification count
- `UNREAD:{user_address}:{platform_id}`: Platform-specific unread count
- `CHAT:{conversation_id}`: Conversation messages
- `STREAM:CHAT:{user_address}`: Redis Stream for real-time message delivery

## Redpanda Topics

- `events.like.created`: Like events
- `events.comment.created`: Comment events
- `events.message.created`: Message events
- `notifications.delivery`: Delivery jobs
- `delivery.apns`: APNs delivery queue
- `delivery.fcm`: FCM delivery queue
- `delivery.email`: Email delivery queue

## API Endpoints

### Notifications

- `GET /api/v1/notifications?user_address={addr}&platform_id={pid}&limit={n}&offset={n}`: Get notifications (supports platform filtering)
- `GET /api/v1/notifications/counts?user_address={addr}&platform_id={pid}`: Get unread notification counts (total and per-platform)
- `POST /api/v1/notifications/:id/read`: Mark notification as read

### Messages

- `GET /api/v1/messages`: Get messages
- `POST /api/v1/messages`: Send message
- `GET /api/v1/conversations`: Get conversations

### Preferences

- `GET /api/v1/preferences`: Get user preferences
- `POST /api/v1/preferences`: Update preferences

### Device Tokens

- `POST /api/v1/device-tokens`: Register device token for push notifications

### WebSocket

- `GET /ws?user_address={addr}`: WebSocket connection for real-time updates

### Health

- `GET /health`: Health check endpoint

## Configuration

### Environment Variables

#### Database
- `DATABASE_URL`: PostgreSQL connection string
- `DATABASE_MAX_CONNECTIONS`: Max DB connections (default: 10)

#### Redis
- `REDIS_URL`: Redis connection string
- `REDIS_MAX_CONNECTIONS`: Max Redis connections (default: 10)

#### Redpanda/Kafka
- `REDPANDA_BROKERS`: Comma-separated list of brokers (e.g., `localhost:9092`)
- `REDPANDA_CONSUMER_GROUP`: Consumer group name

#### Server
- `API_PORT` or `PORT`: API server port (default: 8080)
- `WS_PORT`: WebSocket port (default: 8081)
- `SERVER_HOST`: Server host (default: 0.0.0.0)

#### Global Delivery Config (Fallback)
- `APNS_BUNDLE_ID`: iOS bundle ID
- `APNS_KEY_ID`: APNs key ID
- `APNS_TEAM_ID`: APNs team ID
- `APNS_KEY_PATH`: Path to APNs .p8 key file (or use `APNS_KEY_CONTENT`)
- `APNS_KEY_CONTENT`: Base64-encoded APNs key content (alternative to `APNS_KEY_PATH`)
- `FCM_SERVER_KEY`: FCM server key
- `RESEND_API_KEY`: Resend API key
- `RESEND_FROM_EMAIL`: Resend sender email address

**Note**: Platform-specific delivery configuration should be stored in the `platform_delivery_config` table. Global config is used as a fallback for MySocial platform notifications or when platform config is missing.

## Setup

### Prerequisites

- Rust 1.81+
- PostgreSQL 14+
- Redis 6+
- Redpanda or Kafka

### Database Migrations

Run migrations to set up the database schema:

```bash
cd crates/mys-social-indexer
diesel migration run
```

### Building

```bash
cd mys-relay
cargo build --release
```

### Running

```bash
cd mys-relay
cargo run --bin relay-runner
```

Or use the Dockerfile:

```bash
docker build -t mys-relay .
docker run -p 8080:8080 mys-relay
```

## Deployment

### Railway

The project includes a `railway.toml` configuration file for Railway deployment:

```toml
[build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[deploy]
startCommand = "relay"
healthcheckPath = "/health"
healthcheckTimeout = 100
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 10
```

**Note**: The Dockerfile expects to be run from the parent directory (`crates/mys-social-indexer`). Set the Railway root directory accordingly or adjust the Dockerfile paths.

## Platform Configuration

To configure delivery settings for a platform:

```sql
INSERT INTO platform_delivery_config (
    platform_id,
    apns_bundle_id,
    apns_key_id,
    apns_team_id,
    apns_key_content,  -- Base64-encoded .p8 key content
    fcm_server_key,
    resend_api_key,
    resend_from_email
) VALUES (
    'your-platform-id',
    'com.example.app',
    'ABC123XYZ',
    'TEAM123',
    'base64-encoded-key-content',
    'fcm-server-key',
    'resend-api-key',
    'noreply@example.com'
);
```

## Notification Flow

1. **Indexer** writes events to `relay_outbox` table
2. **Outbox Poller** reads unprocessed events and publishes to Redpanda topics
3. **Notification Service** consumes events, creates notifications, stores in Postgres/Redis
4. **Notification Service** increments unread counts (total and platform-specific)
5. **Notification Service** emits delivery job to `notifications.delivery` topic
6. **Delivery Service** consumes delivery jobs, looks up platform config, and sends via APNs/FCM/Email
7. **API Server** serves notifications via REST API and WebSocket

## Development

### Project Structure

```
mys-relay/
├── relay-core/          # Shared core functionality
├── relay-api/           # REST API and WebSocket server
├── relay-notify/        # Notification processing
├── relay-messaging/     # Messaging service
├── relay-delivery/      # Delivery workers (APNs/FCM/Email)
├── relay-outbox/        # CDC poller
├── relay-runner/        # Main binary (spawns all services)
├── Cargo.toml          # Workspace configuration
├── Dockerfile          # Docker build configuration
└── railway.toml        # Railway deployment config
```

### Testing

```bash
cargo test --workspace
```

### Linting

```bash
cargo clippy --workspace
```

## License

Apache-2.0

