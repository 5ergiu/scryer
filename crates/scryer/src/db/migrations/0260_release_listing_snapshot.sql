-- Import-time scoring and the re-derived incumbent bar must read the same
-- indexer-reported listing facts (published date, votes, password flag,
-- languages, plugin extras) the grab decision read. Those facts are not
-- recoverable from the release title, so the grab freezes them as an opaque
-- JSON snapshot that travels with the submission, the parked pending release,
-- and the imported media file.
-- Nullable, no backfill: a row written before this column carries no snapshot.
ALTER TABLE download_submissions
    ADD COLUMN release_listing_json TEXT;

ALTER TABLE pending_releases
    ADD COLUMN release_listing_json TEXT;

ALTER TABLE media_files
    ADD COLUMN release_listing_json TEXT;
