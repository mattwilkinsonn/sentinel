-- Add tribe and system name columns to threat profiles
ALTER TABLE threat_profiles ADD COLUMN IF NOT EXISTS tribe_id TEXT NOT NULL DEFAULT '';
ALTER TABLE threat_profiles ADD COLUMN IF NOT EXISTS tribe_name TEXT NOT NULL DEFAULT '';
ALTER TABLE threat_profiles ADD COLUMN IF NOT EXISTS last_seen_system_name TEXT NOT NULL DEFAULT '';

-- Cache tables for World API data (avoid re-fetching on restart)
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
