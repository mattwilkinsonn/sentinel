-- Threat profiles: the core persistent entity
CREATE TABLE IF NOT EXISTS threat_profiles (
    character_item_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    threat_score BIGINT NOT NULL DEFAULT 0,
    kill_count BIGINT NOT NULL DEFAULT 0,
    death_count BIGINT NOT NULL DEFAULT 0,
    bounty_count BIGINT NOT NULL DEFAULT 0,
    last_kill_timestamp BIGINT NOT NULL DEFAULT 0,
    last_seen_system TEXT NOT NULL DEFAULT '',
    last_seen_system_name TEXT NOT NULL DEFAULT '',
    tribe_id TEXT NOT NULL DEFAULT '',
    tribe_name TEXT NOT NULL DEFAULT '',
    recent_kills_24h BIGINT NOT NULL DEFAULT 0,
    systems_visited BIGINT NOT NULL DEFAULT 0,
    published_score BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Raw events: recent audit trail (auto-pruned by app)
CREATE TABLE IF NOT EXISTS raw_events (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    timestamp_ms BIGINT NOT NULL,
    data JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp ON raw_events (timestamp_ms DESC);
CREATE INDEX IF NOT EXISTS idx_events_type ON raw_events (event_type);

-- Checkpoint cursor: tracks last processed checkpoint
CREATE TABLE IF NOT EXISTS checkpoint_cursor (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    last_checkpoint BIGINT NOT NULL
);

-- Cache tables for World API data
CREATE TABLE IF NOT EXISTS solar_system_cache (
    system_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS tribe_cache (
    tribe_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    name_short TEXT NOT NULL DEFAULT '',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Character name cache
CREATE TABLE IF NOT EXISTS character_name_cache (
    character_item_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Gate name cache
CREATE TABLE IF NOT EXISTS gate_name_cache (
    gate_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
