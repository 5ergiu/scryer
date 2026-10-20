-- The movie availability gate settings move from instance-wide acquisition
-- keys to the Movie facet, where libraries can override them. Each instance
-- value becomes the Movie facet value; the acquisition keys are retired.
INSERT INTO settings_definitions
    (id, category, scope, key_name, data_type, default_value_json, is_sensitive, created_at, updated_at)
VALUES
    ('media:system:movie.default_availability', 'media', 'system', 'movie.default_availability',
     'string', '"released"', 0,
     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('media:system:movie.release_market', 'media', 'system', 'movie.release_market',
     'string', '"US"', 0,
     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('media:system:movie.availability_delay_days', 'media', 'system', 'movie.availability_delay_days',
     'number', '0', 0,
     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
ON CONFLICT(category, scope, key_name) DO NOTHING;

INSERT INTO settings_values
    (id, setting_definition_id, scope, scope_id, value_json, source, updated_by_user_id,
     created_at, updated_at)
SELECT
    new_definition.id || ':movie',
    new_definition.id,
    old_value.scope,
    'movie',
    old_value.value_json,
    old_value.source,
    old_value.updated_by_user_id,
    old_value.created_at,
    old_value.updated_at
FROM settings_values AS old_value
JOIN settings_definitions AS old_definition
    ON old_definition.id = old_value.setting_definition_id
JOIN settings_definitions AS new_definition
    ON new_definition.scope = old_definition.scope
   AND new_definition.key_name = CASE old_definition.key_name
        WHEN 'acquisition.default_movie_availability' THEN 'movie.default_availability'
        WHEN 'acquisition.movie_release_market' THEN 'movie.release_market'
        WHEN 'acquisition.movie_availability_delay_days' THEN 'movie.availability_delay_days'
    END
WHERE old_definition.key_name IN (
        'acquisition.default_movie_availability',
        'acquisition.movie_release_market',
        'acquisition.movie_availability_delay_days'
    )
  AND old_value.scope_id IS NULL
  AND NOT EXISTS (
        SELECT 1
        FROM settings_values AS existing
        WHERE existing.setting_definition_id = new_definition.id
          AND existing.scope = old_value.scope
          AND existing.scope_id = 'movie'
    );

DELETE FROM settings_values
WHERE setting_definition_id IN (
    SELECT id
    FROM settings_definitions
    WHERE key_name IN (
        'acquisition.default_movie_availability',
        'acquisition.movie_release_market',
        'acquisition.movie_availability_delay_days'
    )
);

DELETE FROM settings_definitions
WHERE key_name IN (
    'acquisition.default_movie_availability',
    'acquisition.movie_release_market',
    'acquisition.movie_availability_delay_days'
);
