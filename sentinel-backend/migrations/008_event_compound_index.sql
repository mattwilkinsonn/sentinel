-- Compound index for efficient event queries filtered by type + sorted by time
CREATE INDEX IF NOT EXISTS idx_events_type_timestamp ON raw_events (event_type, timestamp_ms DESC);
