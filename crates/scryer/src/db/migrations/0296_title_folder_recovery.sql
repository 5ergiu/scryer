ALTER TABLE location_operations ADD COLUMN recovery_key TEXT;
CREATE UNIQUE INDEX idx_location_operations_recovery_key ON location_operations(recovery_key);
ALTER TABLE location_operations ADD COLUMN recovery_committed_at TEXT;
CREATE TABLE location_recovery_staging (
    id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL REFERENCES location_operations(id),
    media_file_id TEXT NOT NULL,
    source_path TEXT NOT NULL,
    destination_path TEXT NOT NULL,
    staging_path TEXT NOT NULL UNIQUE
);
