-- The retired gateway source cannot sync, but its operator-owned configuration
-- and history must survive. Keep it as a disabled provider source so existing
-- list readers can still display it without reviving gateway fetches.
UPDATE list_subscriptions
SET enabled = FALSE, source_origin = 'provider_fetch', sync_state = 'off', next_sync_at = NULL
WHERE source_origin = 'smg_imdb_list';
