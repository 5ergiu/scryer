-- Request rules are now either disabled or enabled. A rule that only recorded
-- its verdicts comes back disabled, for its author to enable by hand, and the
-- instance-wide switch that gated every rule is gone.
UPDATE request_rule_sets
SET evaluation_mode = 'disabled',
    enabled = 0
WHERE evaluation_mode = 'shadow';

DELETE FROM settings_values
WHERE setting_definition_id IN (
    SELECT id
    FROM settings_definitions
    WHERE key_name = 'request_rules.evaluation_enabled'
);

DELETE FROM settings_definitions
WHERE key_name = 'request_rules.evaluation_enabled';
