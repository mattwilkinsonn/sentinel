# SENTINEL

[![CI](https://github.com/mattwilkinsonn/sentinel/actions/workflows/ci.yml/badge.svg)](https://github.com/mattwilkinsonn/sentinel/actions/workflows/ci.yml)

Decentralized threat intelligence network for
[EVE Frontier](https://evefrontier.com) on
[Sui](https://sui.io).

Streams on-chain events, computes threat scores, publishes
results to an on-chain registry, and provides a real-time
dashboard. Smart Gates can autonomously block high-threat
pilots based on SENTINEL scores.

Built for the EVE Frontier x Sui Hackathon 2026.

## Architecture

```text
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

**On-chain:** Move smart contracts for threat registry
and bounty board.

**Backend:** Rust/Tokio service with true multi-threaded
concurrency. Spawns parallel tasks for gRPC checkpoint
streaming, historical data loading, on-chain publishing,
name/system resolution, DB persistence, and Discord bot —
all running across multiple CPU cores simultaneously. SSE
pushes events to the frontend the instant a checkpoint is
processed.

**Frontend:** Solid.js dashboard with fine-grained reactivity
(no virtual DOM diffing). Real-time threat leaderboard,
event feed, kill stats, system intelligence, and earned titles.

**Discord bot:** Serenity-based bot with slash commands for
querying pilot threat scores, leaderboards, kills, system
activity, and live events. Sends real-time CRITICAL threat
alerts to configured channels.

### Concurrency model

The backend runs multiple Tokio tasks in parallel:

- **gRPC stream** — ingests Sui checkpoints in real-time;
  reconnects automatically on disconnect
- **Historical loader** — seeds profiles and structure data
  from Sui GraphQL on startup; runs in background so the
  API is immediately responsive
- **On-chain publisher** — batches threat scores to the
  ThreatRegistry contract every 30s; only publishes profiles
  that changed beyond a configurable threshold
- **Name resolver** — resolves pending character names via
  gRPC object lookups every 10s
- **Metadata resolver** — resolves system names, tribe
  affiliations, and structure type names via World REST API
- **DB sync loop** — flushes dirty profiles and events to
  Postgres every 5s
- **Discord bot** — runs as an independent Tokio task;
  reads shared `AppState` directly with no extra IPC
- **Health monitor** — logs stream staleness and full
  health summaries; warns if no checkpoint in >2 minutes
- **Demo event loop** — generates realistic fake events
  so the dashboard is usable without live chain activity
- **HTTP server** — serves REST API + SSE concurrently

Unlike single-threaded async runtimes, Tokio distributes
tasks across all available CPU cores. The gRPC stream can
process a checkpoint on one core while the API serves a
request on another — true parallelism, not cooperative
multitasking.

Shared state uses `Arc<RwLock<AppState>>` with a strict
short-lock discipline:

- **Readers never block readers.** Multiple API requests
  read threat data simultaneously with zero contention.
- **Writers hold locks for microseconds.** The gRPC handler
  locks, updates one profile's stats, unlocks — then moves
  to the next checkpoint. No lock is ever held across a
  network call, disk write, or `.await` point.
- **Expensive I/O happens outside locks.** The DB sync loop
  snapshots dirty profiles under a brief write lock, then
  performs all Postgres upserts with no lock held. The
  historical loader resolves gate names via GraphQL
  (network I/O) before acquiring the state lock to insert.
- **Background tasks are independent.** The gRPC stream,
  publisher, metadata resolver, and DB sync loop each
  acquire locks independently — a slow World API response
  in the metadata resolver never blocks the gRPC stream
  from processing the next checkpoint.

Historical data loads in the background so the API is
responsive immediately on startup.

## Tech Stack

| Layer           | Tech                                           |
| --------------- | ---------------------------------------------- |
| Smart Contracts | Move (Sui)                                     |
| Backend         | Rust, Axum, Tokio, SQLx, tonic (gRPC)          |
| Discord Bot     | Serenity (Rust)                                |
| Database        | PostgreSQL 16 (Neon prod, Docker dev)          |
| Frontend        | Solid.js, TailwindCSS 4, Vite                  |
| Infrastructure  | AWS ECS Fargate Spot, API Gateway, CloudFront, Pulumi |
| CI/CD           | GitHub Actions                                 |
| Linting         | Biome (TS), rustfmt (Rust), Prettier (Move)    |

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
just fmt       # Format everything
just lint      # Lint TypeScript (Biome)
just check     # Verify all formatting + linting
```

## Project Structure

```text
sentinel/
  move-contracts/
    sentinel/          # Threat registry + smart gate
    bounty_board/      # Bounty board contract
  sentinel-backend/    # Rust backend service
    src/
      api.rs           # REST API + SSE endpoints
      config.rs        # Environment config (panics on missing vars)
      db.rs            # Postgres persistence + migrations
      demo.rs          # Demo mode (realistic fake events)
      discord.rs       # Discord bot (serenity) — slash commands + alerts
      grpc.rs          # Checkpoint streaming + killmail/event handlers
      historical.rs    # GraphQL historical loader (characters, structures)
      names.rs         # Character name resolution via gRPC
      publisher.rs     # On-chain threat score publisher
      sui_client.rs    # Shared Sui gRPC utilities
      threat_engine.rs # Threat scoring + tiers + earned titles
      types.rs         # AppState, ThreatProfile, DataStore
      world_api.rs     # World REST API (system names, tribes, type names)
    migrations/        # SQL schema (auto-applied on startup)
    tests/             # Integration tests (DB, gRPC mock, GraphQL mock)
  frontend/            # Solid.js dashboard
    src/
      SentinelDashboard.tsx  # Main dashboard
      SentinelFeed.tsx       # Live event feed
      StatsBar.tsx           # Aggregate stats bar
      ThreatCard.tsx         # Pilot threat card
      ThreatLeaderboard.tsx  # Threat leaderboard
      views/                 # Sub-views (feed, pilots, kills, systems)
  ts-scripts/          # Admin scripts
  infrastructure/      # Pulumi IaC (TypeScript)
  .github/workflows/   # CI/CD pipeline
```

## API Endpoints

| Endpoint             | Method | Description                     |
| -------------------- | ------ | ------------------------------- |
| `/api/data`          | GET    | Combined demo + live data       |
| `/api/events/stream` | SSE    | Real-time event stream          |
| `/api/health`        | GET    | Health check + profile counts   |

## Discord Bot

SENTINEL includes a Discord bot for querying threat data and receiving
real-time alerts without opening the dashboard.

### Slash commands

| Command        | Description                                                        |
| -------------- | ------------------------------------------------------------------ |
| `/threat`      | Look up a pilot — threat score, tier, K/D, titles, recent kills    |
| `/leaderboard` | Top 10 pilots by threat score with tier medals                     |
| `/kills`       | Last 10 kills with killer, victim, system, and timestamp           |
| `/systems`     | Top 10 most active solar systems by pilot count                    |
| `/events`      | Last 10 live events from the feed                                  |
| `/stats`       | Aggregate network stats (pilots tracked, kills, bounties)          |
| `/alerts`      | Configure CRITICAL threat alerts for your server                   |

### Alerts

Use `/alerts set #channel` to designate a channel for automatic
CRITICAL-tier threat notifications. SENTINEL posts an embed whenever
a pilot crosses the CRITICAL threshold (score > 75.00), including
their stats, earned titles, and a threat score progress bar.
Alert channel configuration is persisted in Postgres and survives restarts.

### Bot setup

Set `DISCORD_TOKEN` in your environment to a valid bot token. The bot
registers slash commands globally on startup.

## Threat Scoring

Scores range from 0-100.00 (stored internally as 0-10,000
basis points, displayed divided by 100) across five factors:

| Factor           | Max    | Formula              |
| ---------------- | ------ | -------------------- |
| Recency (24h)    | 3,500  | recent_kills * 600   |
| Kill count       | 2,000  | log2(kills+1) * 600  |
| K/D ratio        | 1,500  | kd * 400             |
| Bounties         | 1,500  | bounty_count * 500   |
| Movement         | 500    | systems_visited * 100|

Threat tiers: LOW (0-25), MODERATE (25.01-50),
HIGH (50.01-75), CRITICAL (75.01+).

## AI-Assisted Development

SENTINEL was built during a 2-week hackathon using Claude as a development
partner. The entire backend, frontend, infrastructure, and smart contracts
were written through human-AI collaboration.

### How it worked

[@mattwilkinsonn](https://github.com/mattwilkinsonn) directed all architecture
decisions, technology choices, and technical corrections. Claude handled
implementation — writing code, debugging, and executing the direction given.
Technical decision came from Matt; Claude's role was to
implement it quickly and correctly.

This is not "AI wrote the app." It's closer to having an engineer
who types very fast and never gets tired — but still needs a tech lead to
make the right calls.

### Parallel agents

For independent workstreams, multiple Claude instances ran in parallel:

- One implementing a backend feature while another wrote frontend components
- One debugging a CI failure while another worked on smart contracts
- One writing tests while another refactored the data model

This required careful coordination to avoid merge conflicts on shared files
like `types.rs` and `AppState`.

### Short-held locks

One concrete example of this collaboration was shaping architecture: the
`Arc<RwLock<AppState>>` lock discipline. Claude's initial implementation
held write locks across async I/O. Matt caught this and directed a
rewrite where:

- Expensive operations (GraphQL, DB writes, World API calls) happen outside locks
- Write locks are held for microseconds — only for in-memory state mutations
- The DB sync loop snapshots dirty state under a brief lock, then flushes to Postgres unlocked

This pattern is documented in the Architecture section above and is the
reason the backend handles concurrent load without contention.

### Example prompts

These are real prompts from the development session, showing the kind of
direction that shaped the system:

#### Architecture & technology choices

- *"Work on postgres persistence. Set up docker compose for local testing."*
- *"Is there anything we can do utilizing this better stack?"*
- *"first let's get this deployed. next we can get this on the real net"*

#### Technical corrections

- *"I think it should just fail if no postgres — makes rollbacks easier if AWS catches the container not healthy"*
- *"We shouldn't have default URLs imo, just panic when the URLs aren't set as env vars"*
- *"Waiting seconds is flaky. can we query the endpoint and get a return value instead?"*
- *"Just remove it entirely. no need to support the in-memory when we can easily spin up docker containers."*

#### Bug reports

- *"Frontend still renders kills as ? killed ? and the other fields have [OBJECT OBJECT]"*
- *"Still getting duplicate kills after wiping db and running backend twice"*
- *"Top system card on Live server feed, still showing system ID not system name"*
- *"still 8 chars with pending names. likely a bug still with our resolution somehow."*

#### Design decisions

- *"Use B"* (choosing OIDC over static AWS keys for CI)
- *"I dont want to disable truthy entirely, can we just add an inline comment ignore?"*
- *"Can we remove the ! for tests"* (eliminating non-null assertions)

#### Feature direction

- *"Can we just wait to send the event with a new char name until we resolve that name?"*
- *"Can we confirm they are STRUCTURE kills via the ID range or another method? it would be nice to have them in the live feed"*
- *"Mini live event feed — make it display as many events to match the length of the content next to it"*

#### Infrastructure

- *"Change to ARM. Doesn't GitHub have a container registry we can use?"*
- *"Should the API be under api.sentinel.zireael.dev?"*
- *"Can we just use ARM CI runners instead of cross compilation?"*

## Deployment

### Local (Docker Compose)

```bash
docker compose up --build
```

### AWS (Pulumi)

Deploys automatically via GitHub Actions on push to `main` (production)
and `dev` (dev environment). Infrastructure is defined in
`infrastructure/src/` using Pulumi TypeScript.

```bash
just deploy-preview      # Preview production changes
just deploy-preview-dev  # Preview dev changes
```

Requires Pulumi Cloud access token and AWS/Cloudflare/Neon secrets
configured in GitHub Actions.
