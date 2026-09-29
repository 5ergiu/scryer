ALTER TABLE discovery_titles ADD COLUMN affinity_signals_json TEXT NOT NULL DEFAULT '[]';
UPDATE discovery_sync_state SET next_context_snapshot_eligible_at = NULL, next_public_feed_eligible_at = NULL, dirty_since = CURRENT_TIMESTAMP, dirty_reason_mask = dirty_reason_mask | 1;
CREATE TABLE discovery_presentation_selection (
    scope_key TEXT PRIMARY KEY NOT NULL,
    language TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1
);
ALTER TABLE discovery_sync_runs ADD COLUMN presentation_revision BIGINT NOT NULL DEFAULT 0;

ALTER TABLE title_metadata_tags ADD COLUMN affinity_signals_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE discovery_title_metadata_tags ADD COLUMN affinity_signals_json TEXT NOT NULL DEFAULT '[]';

UPDATE titles SET metadata_hydration_next_attempt_at = CURRENT_TIMESTAMP,
    metadata_hydration_attempt_count = 0
WHERE EXISTS (
    SELECT 1 FROM title_metadata_tags tag
    WHERE tag.title_id = titles.id AND tag.category = 'theme'
      AND tag.affinity_signals_json = '[]'
);
