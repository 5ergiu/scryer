-- PostgreSQL half of the fuzzy-index queue. spellfix1 was always SQLite-only,
-- so there is nothing to retire here; the queue is what gives both dialects
-- the same index and the same matching behaviour.
CREATE TABLE IF NOT EXISTS title_search_index_queue (
    seq BIGSERIAL PRIMARY KEY NOT NULL,
    title_id TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_title_search_index_queue_title_id
    ON title_search_index_queue(title_id);

-- The cascade case, exactly as on SQLite: a deleted title takes its
-- projection rows with it without the Rust projection writer running, so the
-- database claims it for the index itself.
CREATE OR REPLACE FUNCTION title_search_enqueue_deleted_title()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO title_search_index_queue (title_id) VALUES (OLD.id);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS titles_delete_enqueue_fuzzy_index ON titles;
CREATE TRIGGER titles_delete_enqueue_fuzzy_index
AFTER DELETE ON titles
FOR EACH ROW
EXECUTE FUNCTION title_search_enqueue_deleted_title();
