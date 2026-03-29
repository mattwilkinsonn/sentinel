ALTER TABLE threat_profiles ALTER COLUMN name DROP NOT NULL;
ALTER TABLE threat_profiles ALTER COLUMN name SET DEFAULT NULL;
UPDATE threat_profiles SET name = NULL WHERE name LIKE 'Pilot #%' OR name = '';
