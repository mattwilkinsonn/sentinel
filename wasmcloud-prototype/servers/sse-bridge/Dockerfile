FROM rust:1.82-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY servers/sse-bridge servers/sse-bridge
RUN mkdir -p components/threat-engine/src && echo "" > components/threat-engine/src/lib.rs && \
    mkdir -p components/api-handler/src && echo "" > components/api-handler/src/lib.rs && \
    mkdir -p components/publisher/src && echo "" > components/publisher/src/lib.rs && \
    mkdir -p components/name-resolver/src && echo "" > components/name-resolver/src/lib.rs && \
    mkdir -p components/world-api-client/src && echo "" > components/world-api-client/src/lib.rs && \
    mkdir -p components/demo-generator/src && echo "" > components/demo-generator/src/lib.rs && \
    mkdir -p servers/sui-bridge/src && echo 'fn main(){}' > servers/sui-bridge/src/main.rs && \
    mkdir -p servers/discord-bridge/src && echo 'fn main(){}' > servers/discord-bridge/src/main.rs
COPY components/threat-engine/Cargo.toml components/threat-engine/Cargo.toml
COPY components/api-handler/Cargo.toml components/api-handler/Cargo.toml
COPY components/publisher/Cargo.toml components/publisher/Cargo.toml
COPY components/name-resolver/Cargo.toml components/name-resolver/Cargo.toml
COPY components/world-api-client/Cargo.toml components/world-api-client/Cargo.toml
COPY components/demo-generator/Cargo.toml components/demo-generator/Cargo.toml
COPY servers/sui-bridge/Cargo.toml servers/sui-bridge/Cargo.toml
COPY servers/discord-bridge/Cargo.toml servers/discord-bridge/Cargo.toml
RUN cargo build --release -p sse-bridge 2>/dev/null || true
COPY . .
RUN cargo build --release -p sse-bridge

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/sse-bridge /usr/local/bin/sse-bridge
ENTRYPOINT ["sse-bridge"]
