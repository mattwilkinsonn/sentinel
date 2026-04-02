# Sentinel → wasmCloud Migration Design

**Date:** 2026-04-01
**Type:** Experimental parallel prototype
**Status:** Design approved, pending implementation

---

## Overview

An experimental parallel migration of Sentinel's Rust backend to wasmCloud, running
alongside the existing ECS backend without replacing it. The goal is to explore the
full design space of a wasmCloud architecture for Sentinel, use the latest and most
experimental wasmCloud features, and identify wasmCloud ecosystem gaps worth
contributing upstream.

The prototype lives at `sentinel/wasmcloud-prototype/` and does not affect the
production backend.

---

## Approach

**Approach B — wRPC Server Model (Fully Experimental)**

All capability providers are standalone processes (wRPC servers or containers), not
host plugins. The wasmCloud host is purely a Wasm runtime and NATS lattice participant.
Business logic runs as stateless Wasm components. The three wRPC servers exist solely
because first-party wasmCloud providers for gRPC, SSE, and Discord do not yet exist
— not because they represent a different architectural tier.

This distinction matters: `sui-bridge`, `discord-bridge`, and `sse-bridge` are
capability providers in the same category as `keyvalue-nats` or `http-server`. They
handle I/O. The Wasm components handle all business logic.

---

## System Topology

```
External Sources
────────────────────────────────────────────────────────
Sui Fullnode (gRPC)   Discord API   EVE World API   Dashboard

         ↕                ↕               ↕              ↕

wRPC Servers (standalone Rust containers)
────────────────────────────────────────────────────────
sui-bridge            discord-bridge        sse-bridge
gRPC→JetStream        Serenity bot          SSE fan-out

First-Party Providers (wasmCloud, wRPC server model)
────────────────────────────────────────────────────────
http-server   http-client   keyvalue-nats   messaging-nats   secrets-nats-kv

         ↕  wRPC / NATS lattice  ↕

NATS Backbone — JetStream + KV + Control Plane
────────────────────────────────────────────────────────
JetStream:  sentinel.events   sentinel.scores   sentinel.alerts   sentinel.publish-tick
KV Buckets: sentinel.profiles   sentinel.names   sentinel.systems
            sentinel.discord    sentinel.cache.types    sentinel.meta

         ↕  wRPC invocations  ↕

Wasm Components (stateless business logic)
────────────────────────────────────────────────────────
threat-engine   api-handler   publisher   name-resolver   world-api-client   demo-generator
```

---

## Component Decomposition

### Wasm Components

| Component | Responsibility | Triggered by |
|---|---|---|
| `threat-engine` | Consume events, compute threat scores, write profiles to KV, publish score updates | JetStream `sentinel.events` (durable pull consumer) |
| `api-handler` | Serve `GET /api/data`, `GET /api/health`; delegate SSE to `sse-bridge` | HTTP request via `http-server` provider |
| `publisher` | Read dirty profiles from KV, batch on-chain publish to Sui ThreatRegistry | JetStream `sentinel.publish-tick` (30s scheduled tick) |
| `name-resolver` | Resolve character names, write to names KV. Uses World API (HTTP) where available; for IDs requiring Sui gRPC object lookups, publishes to `sentinel.name-requests` — `sui-bridge` handles the gRPC call and writes the result directly to `sentinel.names` KV. | JetStream `sentinel.name-tick` (10s scheduled tick) |
| `world-api-client` | Fetch system names, tribe data, type metadata from World API, write to KV | JetStream `sentinel.name-tick` (shares tick with name-resolver) |
| `demo-generator` | Publish realistic fake events for dashboard testing without live chain | Standalone, publishes to `sentinel.events` |

### wRPC Servers

| Server | Responsibility | WIT interface |
|---|---|---|
| `sui-bridge` | Connect to Sui gRPC checkpoint stream, parse killmails/bounties/gate events, publish to JetStream. Handles auto-reconnect. Also subscribes to `sentinel.name-requests` and resolves character names via gRPC object lookups, writing results to `sentinel.names` KV. | Producer + `sentinel.name-requests` consumer |
| `discord-bridge` | Run Serenity Discord bot, handle slash commands by reading NATS KV directly, subscribe to `sentinel.alerts` for CRITICAL notifications | Implements `sentinel:notifications/alerts` |
| `sse-bridge` | Manage persistent SSE connections, subscribe to `sentinel.scores` JetStream, fan out score updates to connected dashboard clients | Implements `sentinel:sse/server` |

### Why wRPC servers, not Wasm components

These three subsystems require native Rust async runtimes that cannot run inside a
Wasm sandbox:
- `sui-bridge`: long-lived bidirectional gRPC stream with custom reconnection logic
- `discord-bridge`: Serenity async bot runtime
- `sse-bridge`: persistent HTTP push connections

They would be standard first-party providers if those providers existed. See
[Future Contributions](#future-contributions).

---

## WIT Interfaces

### Standard interfaces (existing)

| Interface | Provider | Used by |
|---|---|---|
| `wasi:http/incoming-handler` | http-server | api-handler |
| `wasi:http/outgoing-handler` | http-client | publisher, name-resolver, world-api-client |
| `wasi:keyvalue/store` + `atomics` | keyvalue-nats | all components |
| `wasmcloud:messaging/consumer` + `producer` | messaging-nats | threat-engine, publisher, name-resolver, demo-generator |
| `wasmcloud:secrets/store` | secrets-nats-kv | publisher, discord-bridge |
| `wasi:logging/logger` | host built-in | all components |

### Custom interfaces (defined in this project)

```wit
// sentinel:ingestion/types — shared event types for the ingestion pipeline
package sentinel:ingestion@0.1.0;

interface types {
  record kill-event {
    victim-id: u64,
    attacker-ids: list<u64>,
    system-id: string,
    timestamp: u64,
    ship-type-id: u64,
  }

  record bounty-event {
    target-id: u64,
    poster-id: u64,
    amount-mist: u64,
    timestamp: u64,
  }

  record gate-event {
    pilot-id: u64,
    gate-object-id: string,
    permitted: bool,
    timestamp: u64,
  }

  variant threat-event {
    kill(kill-event),
    bounty(bounty-event),
    gate(gate-event),
  }
}
```

```wit
// sentinel:notifications/alerts — app-level alert dispatch
// discord-bridge implements this interface.
// If a Slack or email provider is added later, it implements the same interface.
package sentinel:notifications@0.1.0;

interface alerts {
  send-alert: func(
    pilot-name: string,
    pilot-id: u64,
    threat-score: u8,
    tier: string,
    system: string,
  ) -> result<_, string>;

  set-channel: func(guild-id: string, channel-id: string) -> result<_, string>;
  remove-channel: func(guild-id: string) -> result<_, string>;
}
```

```wit
// sentinel:sse/server — SSE connection management
// sse-bridge implements this interface.
package sentinel:sse@0.1.0;

interface server {
  // api-handler calls this to register a new SSE connection
  register-connection: func(connection-id: string) -> result<_, string>;
  remove-connection: func(connection-id: string) -> result<_, string>;
}
```

---

## Data Model

### JetStream Streams

| Stream | Subject | Retention | Max Age | Max Msgs | Purpose |
|---|---|---|---|---|---|
| `SENTINEL_EVENTS` | `sentinel.events` | limits | 24h | 5000 | Raw ingested events. Consumed by `threat-engine` (durable pull consumer, competing consumer group for scale). Also readable by `api-handler` for event history. |
| `SENTINEL_SCORES` | `sentinel.scores` | limits | 1h | — | Score update notifications. Consumed by `sse-bridge` for SSE fan-out. |
| `SENTINEL_ALERTS` | `sentinel.alerts` | limits | 1h | — | CRITICAL threshold crossings. Consumed by `discord-bridge`. |
| `SENTINEL_TICKS` | `sentinel.publish-tick` + `sentinel.name-tick` | limits | 5m | — | Scheduled heartbeats triggering `publisher` (30s) and `name-resolver` (10s). Published by NATS scheduled producers. |
| `SENTINEL_NAME_REQUESTS` | `sentinel.name-requests` | workqueue | 1m | — | Name resolution requests from `name-resolver` that require Sui gRPC. Consumed exclusively by `sui-bridge`, which writes results to `sentinel.names` KV. |

`SENTINEL_EVENTS` with `max_msgs: 5000` replaces the `VecDeque<RawEvent>` ring buffer.
JetStream drops the oldest message when the limit is hit, exactly like the current
in-memory behaviour.

`threat-engine` uses a **durable pull consumer with competing consumer group** — if
scaled to multiple instances, each event is only processed once. JetStream handles
distribution automatically.

### KV Buckets

**`sentinel.profiles`** — one entry per pilot

```
key:   "pilot:{item_id}"
value: JSON {
  item_id: u64, name: string, threat_score: u8,
  kills: u32, deaths: u32, bounty_count: u32,
  systems_visited: u32, last_kill_ts: u64,
  tier: string, titles: string[],
  tribe_id: u64 | null, tribe_name: string | null,
  dirty: bool   // set by threat-engine, cleared by publisher after on-chain batch
}
```

**`sentinel.names`** — character name cache
```
key:   "name:{item_id}"
value: "{character_name}"
```

**`sentinel.systems`** — system metadata cache
```
key:   "system:{system_id}"
value: JSON { name: string, region: string, security_class: string }
```

**`sentinel.cache.types`** — structure type name cache
```
key:   "type:{type_id}"
value: "{type_name}"
```

**`sentinel.discord`** — alert channel config (replaces `discord_alert_channels` table)
```
key:   "guild:{guild_id}"
value: "{channel_id}"
```

**`sentinel.meta`** — aggregate stats and publisher cursor
```
key:   "stats.aggregate"
value: JSON { total_kills: u32, total_pilots: u32, last_updated_ts: u64 }

key:   "publisher.cursor"
value: "{last_published_checkpoint}"
```

### No PostgreSQL

This prototype replaces PostgreSQL entirely with NATS KV + JetStream. All four
current tables map to KV buckets or JetStream streams. The `dirty` flag on profiles
handles the pending-publish pattern that previously relied on a DB query.

---

## wadm Manifest

See `wasmcloud-prototype/sentinel.wadm.yaml` for the full manifest. Key points:

**Named link on `publisher`** — the `http-client` link to Sui RPC is named
`sui-rpc-testnet`. To switch to mainnet: `wash link put --link-name sui-rpc-mainnet`
with the mainnet URL. The component calls `set_link_name()` at runtime — zero
redeployment.

**`daemonscaler` on all providers** — one provider instance per host. Components on
a given host always hit a local provider, no cross-host I/O hops.

**`spreadscaler` on all components** — N total instances distributed across available
hosts by wadm.

---

## Health Checks

| Current | wasmCloud equivalent |
|---|---|
| `/api/health` ECS check | Same endpoint, served by `api-handler` component |
| DB sync loop | Gone — writes are immediate via keyvalue provider |
| gRPC reconnect health | `sui-bridge` container liveness probe (Docker/K8s) |
| Publisher loop | NATS scheduled tick + wadm reconciliation |
| ECS restart on crash | wadm spreadscaler reconciliation |

---

## Secrets

**Backend:** `secrets-nats-kv` — stores secrets encrypted in NATS JetStream KV.
Zero additional infrastructure. Appropriate for a prototype.

**Secrets defined:**

| Secret | Scoping | Used by |
|---|---|---|
| `sui-publisher-key` | Entity-scoped | `publisher` component |
| `discord-bot-token` | Entity-scoped | `discord-bridge` wRPC server |

Components access secrets via `wasmcloud:secrets/store` WIT interface. Secrets are
never in the manifest, never in environment variables, never logged.

**Production note:** Swap to `secrets-vault` (HashiCorp Vault OSS, KV v2, JWT auth)
or `secrets-kubernetes` (K8s Secrets API) without changing any component code —
update the policy reference in the wadm manifest only.

---

## Observability

wasmCloud emits distributed OTel traces across the entire lattice automatically.
No instrumentation code needed in components.

**Trace path for a killmail event:**
```
sui-bridge: publish to sentinel.events
  └─ threat-engine: handle message
       ├─ keyvalue-nats: get profile
       ├─ [scoring computation]
       ├─ keyvalue-nats: set profile
       └─ messaging-nats: publish to sentinel.scores
            └─ sse-bridge: fan out to SSE clients

threat-engine: publish to sentinel.alerts (on CRITICAL)
  └─ discord-bridge: send Discord message
```

**Configuration:** `WASMCLOUD_ENABLE_OBSERVABILITY=true` on the host. Signals routed
to a local OTel Collector → Jaeger for the prototype. Swap the exporter for
CloudWatch, Grafana Cloud, or Honeycomb in production.

---

## Deployment

### Local (prototype validation)

```bash
# Start infrastructure + wasmCloud
docker compose up nats wadm wasmcloud otel-collector jaeger

# Deploy application
wash app deploy wasmcloud-prototype/sentinel.wadm.yaml

# Start wRPC servers
docker compose up sui-bridge discord-bridge sse-bridge

# Watch reconciliation
wash app list

# Jaeger UI
open http://localhost:16686
```

### Cloud: Cosmonic Control on EKS

Cosmonic Control is the managed control plane from the wasmCloud authors. It installs
onto a Kubernetes cluster via Helm and manages wasmCloud hosts, NATS, and application
lifecycle. Uses Kubernetes CRDs instead of standard wadm YAML.

```bash
# Create EKS cluster
eksctl create cluster --name sentinel-wasmcloud --region us-east-1 --nodes 2

# Install Cosmonic Control
helm install cosmonic-control oci://ghcr.io/cosmonic/cosmonic-control \
  --version 0.3.0 \
  --namespace cosmonic-system \
  --create-namespace

# Install host group
helm install sentinel-hosts oci://ghcr.io/cosmonic/cosmonic-control-hostgroup \
  --namespace cosmonic-system

# Deploy application (Cosmonic CRD format — see wasmcloud-prototype/cosmonic/)
kubectl apply -f wasmcloud-prototype/cosmonic/
```

The `sentinel.wadm.yaml` manifest is the reference spec. The Cosmonic CRD format
translation lives in `wasmcloud-prototype/cosmonic/`.

**Note:** Cosmonic uses its own K8s CRDs (`Component`, `HTTPTrigger`, `CronTrigger`,
etc.) rather than standard wadm YAML. The wadm manifest remains the source of truth
for the component/link/trait design; the Cosmonic manifests are a deployment target
translation.

### Future: Cosmonic Managed SaaS

Cosmonic's fully managed hosted service is on their roadmap (no launch date as of
2026-04-01). When available, this becomes the simplest deployment path — no K8s
cluster to manage.

---

## Directory Structure

```
sentinel/
└── wasmcloud-prototype/
    ├── wit/
    │   ├── sentinel-ingestion.wit     # sentinel:ingestion/types
    │   ├── sentinel-notifications.wit # sentinel:notifications/alerts
    │   └── sentinel-sse.wit           # sentinel:sse/server
    ├── components/
    │   ├── threat-engine/             # Rust Wasm component
    │   ├── api-handler/               # Rust Wasm component
    │   ├── publisher/                 # Rust Wasm component
    │   ├── name-resolver/             # Rust Wasm component
    │   ├── world-api-client/          # Rust Wasm component
    │   └── demo-generator/            # Rust Wasm component
    ├── servers/
    │   ├── sui-bridge/                # Standalone Rust binary (wRPC server)
    │   ├── discord-bridge/            # Standalone Rust binary (wRPC server)
    │   └── sse-bridge/                # Standalone Rust binary (wRPC server)
    ├── sentinel.wadm.yaml             # Declarative application manifest
    ├── cosmonic/                      # Cosmonic CRD translations
    │   ├── threat-engine.yaml
    │   ├── api-handler.yaml
    │   └── ...
    ├── otel-config.yaml               # OTel Collector config (local)
    └── docker-compose.yml             # Local dev environment
```

---

## Future Contributions

Gaps in the wasmCloud ecosystem identified during this design, planned as upstream
contributions:

### 1. `secrets-aws` backend
Implement an AWS Secrets Manager secrets backend following the wasmCloud secrets
protocol (XKey encryption, `wasmcloud.secrets.v1alpha1.<name>.get` NATS subjects,
JWKS-based auth). Currently documented as an example in wasmCloud docs but no
implementation exists. Would complement `secrets-vault` and `secrets-kubernetes`.

### 2. `grpc-client` capability provider
A first-party wasmCloud provider exposing a `wasi:grpc/client` (or `wasmcloud:grpc`)
WIT interface for making gRPC calls from Wasm components. Would eliminate the need
for `sui-bridge` as a custom wRPC server — the gRPC stream would become a standard
provider link. Significant ecosystem value for any blockchain or internal service
integration.

### 3. `sse-server` capability provider
A first-party provider handling persistent SSE connections, exposing a
`wasmcloud:sse/server` WIT interface. Complements `http-server` for push-based
transports. Would eliminate `sse-bridge`.

### 4. `wasmcloud:notifications` provider
A generalised notification/alert dispatch provider with pluggable backends (Discord,
Slack, email, webhook). Would expose a `wasmcloud:notifications/alerts` WIT interface
and eliminate `discord-bridge`. The `sentinel:notifications/alerts` interface designed
here is a direct prototype for the upstream interface.
