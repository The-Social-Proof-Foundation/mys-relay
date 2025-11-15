FROM rust:1.81 as build

WORKDIR /app

# Copy workspace files
COPY mys-relay/Cargo.toml mys-relay/Cargo.toml
COPY mys-relay/relay-core/Cargo.toml mys-relay/relay-core/Cargo.toml
COPY mys-relay/relay-api/Cargo.toml mys-relay/relay-api/Cargo.toml
COPY mys-relay/relay-notify/Cargo.toml mys-relay/relay-notify/Cargo.toml
COPY mys-relay/relay-messaging/Cargo.toml mys-relay/relay-messaging/Cargo.toml
COPY mys-relay/relay-delivery/Cargo.toml mys-relay/relay-delivery/Cargo.toml
COPY mys-relay/relay-outbox/Cargo.toml mys-relay/relay-outbox/Cargo.toml
COPY mys-relay/relay-runner/Cargo.toml mys-relay/relay-runner/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p mys-relay/relay-core/src mys-relay/relay-api/src mys-relay/relay-notify/src \
    mys-relay/relay-messaging/src mys-relay/relay-delivery/src mys-relay/relay-outbox/src \
    mys-relay/relay-runner/src && \
    echo "fn main() {}" > mys-relay/relay-runner/src/main.rs && \
    echo "" > mys-relay/relay-core/src/lib.rs && \
    echo "" > mys-relay/relay-api/src/lib.rs && \
    echo "" > mys-relay/relay-notify/src/lib.rs && \
    echo "" > mys-relay/relay-messaging/src/lib.rs && \
    echo "" > mys-relay/relay-delivery/src/lib.rs && \
    echo "" > mys-relay/relay-outbox/src/lib.rs

# Copy workspace Cargo.toml
COPY Cargo.toml Cargo.toml

# Build dependencies only (for caching)
RUN cd mys-relay && cargo build --release --bin relay-runner || true

# Copy actual source code
COPY mys-relay/ mys-relay/

# Build the binary
RUN cd mys-relay && cargo build --release --bin relay-runner

FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary
COPY --from=build /app/mys-relay/target/release/relay-runner /usr/local/bin/relay

CMD ["relay"]

