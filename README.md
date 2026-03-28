# SENTINEL

Decentralized threat intelligence network for [EVE Frontier](https://evefrontier.com) on [Sui](https://sui.io).

Streams on-chain events, computes threat scores, publishes results to an on-chain registry, and provides a real-time dashboard. Smart Gates can autonomously block high-threat pilots based on SENTINEL scores.

Built for the EVE Frontier x Sui Hackathon 2026.

## Architecture

```
Sui Blockchain (gRPC checkpoint stream)
        |
        v
+------------------+      +------------------+
| Rust Backend     |----->| Postgres         |
| (Axum + Tokio)   |      | (Neon / Docker)  |
+------------------+      +------------------+
   |  REST + SSE
   v
+------------------+
| Solid.js         |
| Dashboard        |
+------------------+
```

**On-chain:** Move smart contracts for threat registry and bounty board.
**Backend:** Rust service ingests checkpoints via gRPC, scores threats, persists to Postgres, serves REST API + SSE.
**Frontend:** Solid.js dashboard with real-time threat leaderboard, event feed, kill stats, and system intelligence.

## Tech Stack

| Layer | Tech |
|-------|------|
| Smart Contracts | Move (Sui) |
| Backend | Rust, Axum, Tokio, SQLx, tonic (gRPC) |
| Database | PostgreSQL 16 (Neon serverless in prod, Docker locally) |
| Frontend | Solid.js, TailwindCSS 4, Vite |
| Infrastructure | AWS ECS Fargate, CloudFront, SST |
| CI/CD | GitHub Actions |
| Linting | Biome (TypeScript), rustfmt (Rust), Prettier (Move) |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/)
- [Bun](https://bun.sh/)
- [Docker](https://docs.docker.com/get-docker/)
- [Just](https://github.com/casey/just)
- [Sui CLI](https://docs.sui.io/guides/developer/getting-started/sui-install)
- [pre-commit](https://pre-commit.com/)

### Setup

```bash
# Install dependencies
just install

# Install git hooks
pre-commit install

# Copy and fill in environment variables
cp .env.example .env
```

### Development

```bash
# Start Postgres + backend + frontend
just dev

# Backend: http://localhost:3001
# Frontend: http://localhost:5173
```

Or run components individually:

```bash
just db              # Start Postgres
just backend-run     # Start backend (needs DATABASE_URL)
just frontend-dev    # Start frontend dev server
```

### Testing

```bash
just backend-test              # Rust unit tests
just backend-test-integration  # Postgres integration tests
just frontend-test             # Frontend tests
just contracts-test            # Move contract tests
```

### Code Quality

```bash
just fmt       # Format everything (Rust + TypeScript + Move)
just lint      # Lint TypeScript (Biome)
just check     # Verify all formatting + linting passes
```

## Project Structure

```
sentinel/
  move-contracts/
    sentinel/          # Threat registry + smart gate contracts
    bounty_board/      # Bounty board contract
  sentinel-backend/    # Rust backend service
    src/
      api.rs           # REST API + SSE endpoints
      grpc.rs          # Sui checkpoint streaming + event handlers
      threat_engine.rs # Threat scoring algorithm
      db.rs            # Postgres persistence
      demo.rs          # Demo mode (fake data for testing)
      publisher.rs     # On-chain score publisher (WIP)
    migrations/        # SQL schema
    tests/             # Integration tests
  frontend/            # Solid.js dashboard
    src/
      SentinelDashboard.tsx  # Main dashboard
      BountyBoard.tsx        # Bounty board UI
      views/                 # Sub-views (feed, kills, systems, tracked)
  ts-scripts/          # Admin scripts (deploy, configure, bounties)
  infrastructure/      # SST deployment config
  .github/workflows/   # CI/CD pipeline
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/data` | GET | Combined demo + live threat data |
| `/api/events/stream` | SSE | Real-time event stream |
| `/api/health` | GET | Health check with profile/checkpoint counts |

## Threat Scoring

Scores range from 0-10,000 basis points across five factors:

| Factor | Max Points | Formula |
|--------|-----------|---------|
| Recency (24h kills) | 3,500 | recent_kills * 600 |
| Kill count | 2,000 | log2(kills+1) * 600 |
| K/D ratio | 1,500 | kd * 400 |
| Bounties | 1,500 | bounty_count * 500 |
| Movement | 500 | systems_visited * 100 |

Threat tiers: LOW (0-2500), MODERATE (2501-5000), HIGH (5001-7500), CRITICAL (7501+).

## Deployment

### Local (Docker Compose)

```bash
docker compose up --build
```

### AWS (SST)

```bash
just deploy-dev   # Deploy to dev stage
just deploy       # Deploy to production
```

Requires AWS credentials and GitHub secrets configured. See `infrastructure/sst.config.ts` for resource definitions.

## License

MIT
