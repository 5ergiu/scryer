-- Let an episode's acquisition-state row carry its owning collection id. See
-- the SQLite twin for why the 0054 index could not hold both of
-- `wanted_items.collection_id`'s jobs.
DROP INDEX IF EXISTS idx_wanted_items_collection_id;

CREATE UNIQUE INDEX idx_wanted_items_collection_id
    ON wanted_items USING btree (collection_id)
    WHERE (collection_id IS NOT NULL AND episode_id IS NULL);
