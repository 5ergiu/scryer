-- Look up a title by external id across every library.
--
-- Every writer stores `source` lowercased and `external_id` trimmed, but keeps
-- the id's own case, and a cross-library lookup compares the id without case.
-- Both existing indexes lead with `library_id` and hold the id as written, so
-- that lookup scanned the whole table, once per id the release carried.
CREATE INDEX IF NOT EXISTS idx_title_external_ids_source_value_nocase
    ON title_external_ids(source, LOWER(external_id));
