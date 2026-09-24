CREATE INDEX IF NOT EXISTS idx_download_import_artifacts_import_episode
ON download_import_artifacts (import_id, episode_id, result, created_at DESC);
