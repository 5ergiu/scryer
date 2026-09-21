-- Index the case- and separator-insensitive half of the folder-ownership
-- lookup.
--
-- On Windows `folder_paths_match` treats `/` and `\` as one separator and
-- ignores case, so the narrowed read added in 0249 has to compare
-- `lower(replace(folder_path, '/', '\'))` as well as the raw column, or a title
-- stored as `C:\Media\Show` is invisible to a scan that supplies
-- `c:/media/show` and a second title claims a folder that already has an owner.
-- `idx_titles_library_folder_path` cannot answer that predicate, so without this
-- the folded arm scans every title row of the library once per folder a scan
-- touches.
--
-- The expression is derived by the engine, so there is nothing to backfill and
-- nothing to go stale when the same database is opened from another platform —
-- which is why this is an expression index and not a persisted identity column:
-- the identity rule itself is platform-dependent, so a stored key written on
-- Windows would be wrong the moment the database moved to Linux.

CREATE INDEX IF NOT EXISTS idx_titles_library_folder_path_lookup
    ON titles(library_id, lower(replace(folder_path, '/', '\')));
