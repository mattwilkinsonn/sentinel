# SENTINEL — EVE Frontier Threat Intelligence Network

export PATH := env("HOME") / ".bun/bin:" + env("HOME") / ".local/bin:" + env("HOME") / ".cargo/bin:" + env("PATH")

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
    cd sentinel-backend && DATABASE_URL=postgresql://sentinel:sentinel@localhost/sentinel cargo test --test db_integration

# Run gRPC mock tests (no external dependencies)
backend-test-grpc:
    cd sentinel-backend && cargo test --test grpc_mock_tests

# Run gRPC integration tests against real Sui testnet
backend-test-grpc-live:
    cd sentinel-backend && cargo test --test grpc_integration_tests -- --ignored

# Build backend (release)
backend-build:
    cd sentinel-backend && cargo build --release

# Run backend service (watches for changes)
backend-run:
    cd sentinel-backend && cargo watch -x run

# === Frontend ===

# Install frontend dependencies
frontend-install:
    cd frontend && bun install

# Run frontend dev server (waits for backend if not ready)
frontend-dev:
    @until curl -sf http://localhost:3001/api/health > /dev/null 2>&1; do sleep 1; done
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

# Install all dependencies
install: frontend-install scripts-install

# Run all tests
test: backend-test frontend-test contracts-test

# Build everything
build: contracts-build backend-build frontend-build

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
dev: db
    @echo "Starting backend on :3001 and frontend on :5173..."
    @DATABASE_URL=postgresql://sentinel:sentinel@localhost/sentinel just backend-run &
    @echo "Waiting for backend..."
    @until curl -sf http://localhost:3001/api/health > /dev/null 2>&1; do sleep 1; done
    @echo "Backend ready!"
    @just frontend-dev

# Run backend + Postgres in Docker (rebuilds on source changes)
dev-docker:
    docker compose up --build --watch

# Alias for frontend dev.
dev-frontend: frontend-dev
