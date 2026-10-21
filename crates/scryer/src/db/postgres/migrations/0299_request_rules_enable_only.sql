-- Request rules are now either disabled or enabled. A rule that only recorded
-- its verdicts comes back disabled, for its author to enable by hand, and the
-- instance-wide switch that gated every rule is gone.
UPDATE request_rule_sets
SET evaluation_mode = 'disabled',
    enabled = false
WHERE evaluation_mode = 'shadow';

-- The switch defaulted to off, and while it was off no rule acted. An
-- enforcing rule on an instance that never turned the switch on would
-- otherwise start deciding requests on upgrade, so it comes back disabled too.
UPDATE request_rule_sets
SET evaluation_mode = 'disabled',
    enabled = false
WHERE evaluation_mode = 'enforce'
  AND NOT EXISTS (
      SELECT 1
      FROM settings_values v
      JOIN settings_definitions d ON d.id = v.setting_definition_id
      WHERE d.key_name = 'request_rules.evaluation_enabled'
        AND v.scope_id IS NULL
        AND lower(trim(both '" ' from v.value_json::text)) IN ('true', '1', 'yes', 'on')
  );

DELETE FROM settings_values
WHERE setting_definition_id IN (
    SELECT id
    FROM settings_definitions
    WHERE key_name = 'request_rules.evaluation_enabled'
);

DELETE FROM settings_definitions
WHERE key_name = 'request_rules.evaluation_enabled';
