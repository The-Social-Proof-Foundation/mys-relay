FROM rust:1.84 as build

# Install build dependencies required for rdkafka-sys
RUN apt-get update && \
    apt-get install -y \
    cmake \
    pkg-config \
    libssl-dev \
    zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY relay-core/Cargo.toml relay-core/Cargo.toml
COPY relay-api/Cargo.toml relay-api/Cargo.toml
COPY relay-notify/Cargo.toml relay-notify/Cargo.toml
COPY relay-messaging/Cargo.toml relay-messaging/Cargo.toml
COPY relay-delivery/Cargo.toml relay-delivery/Cargo.toml
COPY relay-outbox/Cargo.toml relay-outbox/Cargo.toml
COPY relay-runner/Cargo.toml relay-runner/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p relay-core/src relay-api/src relay-notify/src \
    relay-messaging/src relay-delivery/src relay-outbox/src \
    relay-runner/src && \
    echo "fn main() {}" > relay-runner/src/main.rs && \
    echo "" > relay-core/src/lib.rs && \
    echo "" > relay-api/src/lib.rs && \
    echo "" > relay-notify/src/lib.rs && \
    echo "" > relay-messaging/src/lib.rs && \
    echo "" > relay-delivery/src/lib.rs && \
    echo "" > relay-outbox/src/lib.rs

# Build dependencies only (for caching)
RUN cargo build --release --bin relay-runner || true

# Copy actual source code
COPY . .

# Build the binary
RUN cargo build --release --bin relay-runner

FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary
COPY --from=build /app/target/release/relay-runner /usr/local/bin/relay

CMD ["relay"]

