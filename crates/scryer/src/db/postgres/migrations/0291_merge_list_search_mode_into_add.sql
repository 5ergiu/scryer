-- A list's "Add and search" mode merges into "Add": a list now adds each
-- title with its route's monitoring, and monitoring alone decides whether the
-- title is searched. An "Add and search" route that added titles unmonitored
-- searched them anyway, so it moves to the monitoring that keeps that search:
-- future episodes for series and anime, monitored for movies. Every other
-- route keeps its monitoring.
UPDATE list_subscription_routes
SET monitor_type = CASE WHEN kind = 'movie' THEN 'MONITORED' ELSE 'FUTURE_EPISODES' END
WHERE subscription_id IN (
    SELECT id FROM list_subscriptions WHERE lower(trim(mode)) = 'search'
)
AND lower(replace(replace(replace(trim(monitor_type), '_', ''), '-', ''), ' ', ''))
    IN ('unmonitored', 'none');

UPDATE list_subscriptions SET mode = 'add' WHERE lower(trim(mode)) = 'search';
