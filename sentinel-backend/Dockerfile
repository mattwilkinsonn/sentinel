# Build stage
FROM rust:1.94-bookworm AS builder

# Install protoc
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Clone Sui proto definitions
RUN git clone --depth 1 https://github.com/MystenLabs/sui-apis.git proto/sui-apis

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./

# Create dummy src to build dependencies
RUN mkdir -p src migrations && echo 'fn main() {}' > src/main.rs && touch src/lib.rs && touch migrations/001_init.sql
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src migrations

# Copy actual source and migrations
COPY src ./src
COPY migrations ./migrations

# Touch source files so cargo's fingerprint detects the change from the dummy build
RUN find src -name "*.rs" | xargs touch

# Build for real
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sentinel-backend /usr/local/bin/sentinel-backend

EXPOSE 3001

CMD ["sentinel-backend"]
