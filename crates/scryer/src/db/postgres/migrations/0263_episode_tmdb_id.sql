-- TMDB's episode id. Episodes of a TMDB-primary series have no TVDB id, so
-- this is their provider identity; NULL for TVDB-backed episodes and every
-- row written before this migration. Catalog-only: no backfill, no index.
ALTER TABLE episodes ADD COLUMN tmdb_id TEXT;
