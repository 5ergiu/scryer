-- Let an episode's acquisition-state row carry its owning collection id.
--
-- `wanted_items.collection_id` has two jobs: it is the identity of a
-- collection-scoped (season-pack) row, and it is attribution on an
-- episode-scoped row — the shape `find_wanted_state_for_scope` and the
-- in-memory repository have always dispatched on, episode identity first.
-- `idx_wanted_items_collection_id` from 0054 only modelled the first job, so a
-- second episode of the same season could never be written: the store worked
-- around it by matching the collection before the episode, which folded every
-- episode of a season onto one row and let one episode's grab overwrite
-- another's.
--
-- Narrowing the index to the rows that are genuinely collection-scoped keeps
-- season-pack uniqueness and lets the episode rows keep their attribution. It
-- only ever removes rows from the index, so no existing database can conflict
-- on it.

DROP INDEX IF EXISTS idx_wanted_items_collection_id;

CREATE UNIQUE INDEX idx_wanted_items_collection_id
    ON wanted_items(collection_id)
    WHERE collection_id IS NOT NULL AND episode_id IS NULL;
