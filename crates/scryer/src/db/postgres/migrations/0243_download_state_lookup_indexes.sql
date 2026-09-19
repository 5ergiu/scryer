-- Index the two download-state lookups that had to scan. See the SQLite twin
-- for which reads these serve and why the existing indexes do not.
CREATE INDEX IF NOT EXISTS idx_download_identity_states_canonical_latest
    ON download_identity_states(canonical_download_id, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_download_identity_states_client_item
    ON download_identity_states(client_type, download_client_item_id, client_id);

CREATE INDEX IF NOT EXISTS idx_download_submissions_client_item
    ON download_submissions(download_client_type, download_client_item_id, download_client_id);
