-- A download client cuts a file name that will not fit the filesystem, and
-- the cut can drop the episode the import needs. Scryer holds the grab's NZB
-- or torrent only until the client takes it, so the grab keeps the file names
-- long enough to be cut, as JSON, for the import to read a cut name back.
-- Nullable, no backfill: a row written before this column keeps none.
ALTER TABLE download_submissions
    ADD COLUMN grabbed_file_names_json TEXT;
