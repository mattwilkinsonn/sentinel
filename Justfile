# SENTINEL — EVE Frontier Threat Intelligence Network

export PATH := env("HOME") / ".bun/bin:" + env("HOME") / ".local/bin:" + env("HOME") / ".cargo/bin:" + env("PATH")
export DATABASE_URL := "postgresql://sentinel:sentinel@localhost/sentinel"

# Default: list all recipes
default:
    @just --list

# === Contracts ===

# Build sentinel Move contracts
contracts-build:
    cd move-contracts/sentinel && sui move build

# Build bounty board Move contracts
bounty-build:
    cd move-contracts/bounty_board && sui move build

# Test sentinel Move contracts
contracts-test:
    cd move-contracts/sentinel && sui move test

# Deploy sentinel package to testnet
contracts-deploy:
    cd move-contracts/sentinel && sui client publish --build-env testnet

# Deploy bounty board package to testnet (step 1 of 2 — sets BUILDER_PACKAGE_ID and EXTENSION_CONFIG_ID)
bounty-deploy:
    cd move-contracts/bounty_board && sui client publish --build-env testnet

# Create BountyBoard shared object (step 2 of 2 — run after bounty-deploy and setting BUILDER_PACKAGE_ID)
bounty-create-board:
    cd ts-scripts && bun run bounty_board/create-board.ts

# Create threat registry (after deploy)
sentinel-create-registry:
    cd ts-scripts && bun run sentinel/create-registry.ts

# Set gate threshold (needs GATE_MAX_THREAT_SCORE in .env)
sentinel-configure-gate:
    cd ts-scripts && bun run sentinel/configure-gate.ts

# Authorize sentinel on a gate (needs GATE_ID, CHARACTER_ID in .env)
sentinel-authorize-gate:
    cd ts-scripts && bun run sentinel/authorize-gate.ts

# === Backend ===

# Check backend compiles
backend-check:
    cd sentinel-backend && cargo check

# Run backend unit tests
backend-test:
    cd sentinel-backend && cargo test --lib

# Run backend integration tests (requires Postgres)
backend-test-integration: db
    cd sentinel-backend && cargo test --test db_integration --test graphql_mock_tests --test grpc_mock_tests --test logging

# Run integration tests against real Sui testnet (gRPC + GraphQL)
backend-test-live:
    cd sentinel-backend && cargo test --test grpc_integration_tests --test graphql_integration_tests -- --ignored

# Build backend (release)
backend-build:
    cd sentinel-backend && cargo build --release

# Run backend service (watches for changes, starts Postgres if needed)
backend-run: db
    cd sentinel-backend && cargo watch -x run

# === Frontend ===

# Install frontend dependencies
frontend-install:
    cd frontend && bun install

# Run frontend dev server (waits for backend if not ready)
frontend-dev:
    @echo "Waiting for backend on :3001..."
    @until curl -sf http://localhost:3001/api/health > /dev/null 2>&1; do sleep 1; done
    @echo "Backend ready — starting frontend dev server..."
    cd frontend && bun run dev

# Build frontend for production
frontend-build:
    cd frontend && bun run build

# Run frontend tests
frontend-test:
    cd frontend && bun run test

# === Admin Scripts ===

# Install ts-scripts dependencies
scripts-install:
    cd ts-scripts && bun install

# Run an admin script (e.g. just script bounty_board/create-board)
script name:
    cd ts-scripts && bun run {{name}}.ts

# === Formatting & Linting ===

# Format everything
fmt: fmt-rust fmt-ts fmt-move

# Format Rust
fmt-rust:
    cd sentinel-backend && cargo fmt

# Format Move contracts (prettier + Sui plugin)
fmt-move:
    cd ts-scripts && bun run fmt:move

# Format + lint TypeScript (Biome)
fmt-ts:
    bunx biome check --fix --unsafe .

# Lint TypeScript (Biome)
lint:
    bunx biome check .

# Check everything (formatting + linting)
check: check-rust check-ts check-move

# Check Rust formatting
check-rust:
    cd sentinel-backend && cargo fmt -- --check

# Check TypeScript (Biome lint + format)
check-ts:
    bunx biome check .

# Check Move formatting
check-move:
    cd ts-scripts && bun run fmt:move:check

# === Full Stack ===

# Clone Sui gRPC proto definitions (required for backend build)
protos-install:
    mkdir -p sentinel-backend/proto
    @if [ -d "sentinel-backend/proto/sui-apis" ]; then \
        echo "sui-apis already cloned, skipping"; \
    else \
        git clone --depth 1 https://github.com/MystenLabs/sui-apis.git sentinel-backend/proto/sui-apis; \
    fi

# Install all dependencies
install: frontend-install scripts-install protos-install

# Run all tests in parallel with mprocs (each suite in its own pane)
test:
    mprocs \
      --names "unit,integration,live,frontend,contracts" \
      "just backend-test" \
      "just backend-test-integration" \
      "just backend-test-live" \
      "just frontend-test" \
      "just contracts-test"

# Build everything (parallel)
[parallel]
build: contracts-build backend-build frontend-build

# === wasmCloud Prototype ===

# Run all wasmCloud tests (threat-engine scoring + server unit tests)
wc-test:
    cd wasmcloud-prototype && cargo test -p threat-engine -p sui-bridge -p discord-bridge -p sse-bridge

# Check all wasmCloud components and servers compile
wc-check:
    cd wasmcloud-prototype && cargo check -p sui-bridge -p discord-bridge -p sse-bridge
    cd wasmcloud-prototype && cargo check -p threat-engine -p api-handler -p publisher -p name-resolver -p world-api-client -p demo-generator --target wasm32-wasip2

# Build a single Wasm component with wash (e.g. just wc-build-component threat-engine)
wc-build-component name:
    cd wasmcloud-prototype/components/{{name}} && wash build --skip-fetch

# Build all Wasm components with wash build
wc-build-components:
    cd wasmcloud-prototype/components/threat-engine && wash build --skip-fetch
    cd wasmcloud-prototype/components/api-handler && wash build --skip-fetch
    cd wasmcloud-prototype/components/publisher && wash build --skip-fetch
    cd wasmcloud-prototype/components/name-resolver && wash build --skip-fetch
    cd wasmcloud-prototype/components/world-api-client && wash build --skip-fetch
    cd wasmcloud-prototype/components/demo-generator && wash build --skip-fetch

# Build all bridge servers (native)
wc-build-servers:
    cd wasmcloud-prototype && cargo build --release -p sui-bridge -p discord-bridge -p sse-bridge

# Build everything (components + servers)
wc-build: wc-build-components wc-build-servers

# Format wasmCloud Rust code
wc-fmt:
    cd wasmcloud-prototype && cargo fmt --all

# Start observability stack (OTel + Jaeger)
wc-otel:
    cd wasmcloud-prototype && docker compose up otel-collector jaeger -d

# Start bridge servers in Docker
wc-bridges:
    cd wasmcloud-prototype && docker compose up sui-bridge discord-bridge sse-bridge -d

# Start everything: build, then run all services in mprocs
wc-up: wc-build
    mprocs \
      --names "otel,jaeger,sui-bridge,discord-bridge,sse-bridge,wasmcloud-host" \
      "cd wasmcloud-prototype && docker compose up otel-collector" \
      "cd wasmcloud-prototype && docker compose up jaeger" \
      "cd wasmcloud-prototype && cargo run -p sui-bridge" \
      "cd wasmcloud-prototype && cargo run -p discord-bridge" \
      "cd wasmcloud-prototype && cargo run -p sse-bridge" \
      "wash host"

# Stop everything
wc-down:
    cd wasmcloud-prototype && docker compose down

# Stop everything and wipe volumes
wc-reset:
    cd wasmcloud-prototype && docker compose down -v

# View bridge server logs
wc-logs *args="":
    cd wasmcloud-prototype && docker compose logs {{args}}

# Follow logs for a specific service (e.g. just wc-log sui-bridge)
wc-log service:
    cd wasmcloud-prototype && docker compose logs -f {{service}}

# Inspect a built component (e.g. just wc-inspect threat-engine)
wc-inspect name:
    wash inspect wasmcloud-prototype/target/wasm32-wasip2/release/$(echo {{name}} | tr '-' '_').wasm

# Run sui-bridge locally (not in Docker)
wc-run-sui:
    cd wasmcloud-prototype && cargo run -p sui-bridge

# Run discord-bridge locally
wc-run-discord:
    cd wasmcloud-prototype && cargo run -p discord-bridge

# Run sse-bridge locally
wc-run-sse:
    cd wasmcloud-prototype && cargo run -p sse-bridge

# Run demo generator tick locally (publishes one tick to NATS)
wc-demo-tick:
    nats pub sentinel.demo-tick '{}'

# Hot-reload dev loop for a single component (starts embedded host + NATS)
# Good for iterating on one component at a time (e.g. just wc-dev threat-engine)
wc-dev name:
    cd wasmcloud-prototype/components/{{name}} && wash dev

# Run a standalone wasmCloud host (connects to NATS on localhost:4222)
wc-host:
    wash host

# Open Jaeger tracing UI
wc-traces:
    open http://localhost:16686

# === Deploy ===

# Deploy to AWS (production)
deploy:
    cd infrastructure && sst deploy --stage production

# Deploy to AWS (dev/staging)
deploy-dev:
    cd infrastructure && sst deploy --stage dev

# Remove deployment
deploy-remove stage="dev":
    cd infrastructure && sst remove --stage {{stage}}

# === Dev ===

# Start Postgres via docker compose
db:
    docker compose up postgres -d

# Stop Postgres
db-stop:
    docker compose down

# Wipe Postgres data and restart fresh
db-reset:
    docker compose down -v
    docker compose up postgres -d

# Run backend + frontend dev servers (starts Postgres automatically)
dev:
    @echo "Starting backend on :3001 and frontend on :5173..."
    @just backend-run &
    @echo "Waiting for backend..."
    @until curl -sf http://localhost:3001/api/health > /dev/null 2>&1; do sleep 1; done
    @echo "Backend ready!"
    @just frontend-dev

# Run backend + Postgres in Docker (rebuilds on source changes)
dev-docker:
    docker compose up --build --watch

# Alias for frontend dev.
dev-frontend: frontend-dev

dev-backend: backend-run

backend-dev: backend-run
