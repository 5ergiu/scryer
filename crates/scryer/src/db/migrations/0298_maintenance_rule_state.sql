-- Maintenance rules have one control: Disabled, Dry run, or Enabled. Shadow
-- rules already evaluated without acting, which is what Dry run does, so they
-- become Dry run: evaluated and unarmed. Arming is cleared with the mode so a
-- shadow rule that had been armed cannot start acting by becoming evaluated.
UPDATE maintenance_rule_sets
SET evaluation_mode = 'observe',
    effect_arming = 'none'
WHERE evaluation_mode = 'shadow';

-- The presentation effects gate was stored but never consumed. Its values
-- cascade with the definition.
DELETE FROM settings_definitions
WHERE key_name = 'maintenance.gate.presentation_effects';
