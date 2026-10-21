-- Movies now wait on release dates the gateway serves per market. Fetch them
-- promptly for every movie instead of waiting for the ordinary refresh cadence,
-- so a library that just defaulted to 'released' does not sit unsearched.
UPDATE titles
SET metadata_hydration_next_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
    metadata_hydration_attempt_count = 0
WHERE facet = 'movie';
