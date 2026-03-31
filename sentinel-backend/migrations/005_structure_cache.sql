-- Cache mapping structure item_id → type_id (e.g. 1000001092825 → 92279)
-- type_id → name ("Mini Turret") is kept in-memory only (few entries, never changes)
CREATE TABLE IF NOT EXISTS structure_type_cache (
    item_id BIGINT PRIMARY KEY,
    type_id BIGINT NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
