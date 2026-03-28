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
    recent_kills_24h BIGINT NOT NULL DEFAULT 0,
    systems_visited BIGINT NOT NULL DEFAULT 0,
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
