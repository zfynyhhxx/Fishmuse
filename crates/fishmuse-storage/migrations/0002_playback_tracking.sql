ALTER TABLE listening_events ADD COLUMN ended_at INTEGER;
ALTER TABLE listening_events ADD COLUMN interrupted INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0, 1));

UPDATE listening_events SET ended_at = started_at WHERE ended_at IS NULL;
