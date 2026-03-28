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

# === Backend ===

# Check backend compiles
backend-check:
    cd sentinel-backend && cargo check

# Run backend tests
backend-test:
    cd sentinel-backend && cargo test

# Build backend (release)
backend-build:
    cd sentinel-backend && cargo build --release

# Run backend service
backend-run:
    cd sentinel-backend && cargo run

# Run backend with demo data (no blockchain connection needed)
backend-demo:
    cd sentinel-backend && cargo run -- --demo

# === Frontend ===

# Install frontend dependencies
frontend-install:
    cd frontend && bun install

# Run frontend dev server
frontend-dev:
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

# === Formatting ===

# Format Move contracts
fmt-move:
    cd ts-scripts && bun run fmt:move

# Format TypeScript
fmt-ts:
    cd ts-scripts && bun run fmt:ts

# === Full Stack ===

# Install all dependencies
install: frontend-install scripts-install

# Build everything
build: contracts-build backend-build frontend-build

# Run backend + frontend dev servers in parallel
dev:
    @echo "Starting backend on :3001 and frontend on :5173..."
    @just backend-run &
    @just frontend-dev

# Run with demo data (no blockchain connection needed)
dev-demo:
    @echo "Starting demo backend on :3001 and frontend on :5173..."
    @just backend-demo &
    @just frontend-dev
