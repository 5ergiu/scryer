-- SMG's contiguous absolute number: TVDB's absolute order renumbered 1..n over
-- story episodes only. NULL when the series has no absolute order or SMG could
-- not number it safely; absolute_number keeps the raw TVDB value.
ALTER TABLE episodes ADD COLUMN contiguous_absolute_number INTEGER;
