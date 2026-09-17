-- Index the two download-state lookups that had to scan.
--
-- The durable download-history projection resolves the latest
-- `download_identity_states` row per download, and the queue's batch state
-- lookup resolves one per client locator. Neither had an index that answered
-- its shape: the canonical index stops at `canonical_download_id`, so the
-- latest-row pick sorted its matches, and the locator index leads with
-- `client_id` and never covers `download_client_item_id`, so the locator
-- lookups scanned `download_identity_states` and `download_submissions` whole.
--
-- The existing indexes are left in place: they are prefixes of these, and the
-- writes they serve (`download_id` lookups) are not the reads fixed here.

-- Latest identity state per download, in the order the projection picks it.
CREATE INDEX IF NOT EXISTS idx_download_identity_states_canonical_latest
    ON download_identity_states(canonical_download_id, updated_at DESC, id DESC);

-- Client-locator lookups, in the order the predicate constrains them: client
-- type and item id are equality/IN, the client id narrows what is left.
CREATE INDEX IF NOT EXISTS idx_download_identity_states_client_item
    ON download_identity_states(client_type, download_client_item_id, client_id);

CREATE INDEX IF NOT EXISTS idx_download_submissions_client_item
    ON download_submissions(download_client_type, download_client_item_id, download_client_id);
