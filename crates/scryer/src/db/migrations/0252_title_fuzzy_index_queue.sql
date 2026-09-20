-- The fuzzy title lane is a tantivy index now, not a spellfix1 virtual table.
--
-- Two things happen here. First, the queue that keeps that index honest: the
-- projection writer claims a title in the same transaction as its projection
-- rows, so a write that rolls back claims nothing and a write that commits is
-- never missed. Second, the last of spellfix1 leaves the schema.
--
-- The module-backed virtual table itself is removed before any migration
-- runs, by the retirement step in the migration runner: DROP TABLE cannot
-- touch an object whose module is gone, and migrations 0092, 0236 and the
-- 0198 baseline are already applied and checksummed, so they still contain
-- their spellfix statements and still have to replay. That step leaves a
-- plain stand-in table of the same name, which those statements are happy
-- with. This is where the stand-in and the 0236 trigger are dropped.
CREATE TABLE IF NOT EXISTS title_search_index_queue (
    seq INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    title_id TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_title_search_index_queue_title_id
    ON title_search_index_queue(title_id);

DROP TRIGGER IF EXISTS title_search_terms_delete_spellfix;
DROP TABLE IF EXISTS title_search_spellfix;
DROP TABLE IF EXISTS title_search_spellfix_vocab;

-- A title that is deleted takes its projection rows with it through
-- ON DELETE CASCADE, without the Rust projection writer ever running, so the
-- writer's own enqueue cannot see it. spellfix1 kept itself honest for the
-- same case with a trigger; this is that trigger's replacement, and it claims
-- the title once rather than once per term.
DROP TRIGGER IF EXISTS titles_delete_enqueue_fuzzy_index;
CREATE TRIGGER titles_delete_enqueue_fuzzy_index
AFTER DELETE ON titles
BEGIN
    INSERT INTO title_search_index_queue (title_id) VALUES (OLD.id);
END;
