-- Index the case- and separator-insensitive half of the folder-ownership
-- lookup. See the SQLite twin for which read this serves and why
-- `idx_titles_library_folder_path` does not answer it.
CREATE INDEX IF NOT EXISTS idx_titles_library_folder_path_lookup
    ON titles(library_id, lower(replace(folder_path, '/', '\')));
