-- Rebuild the derived search cache before enforcing the new spelling identity.
DELETE FROM title_search_terms;
DROP INDEX IF EXISTS idx_title_search_terms_title_kind_normalized;
DROP INDEX IF EXISTS idx_title_search_terms_title_kind_language;
CREATE UNIQUE INDEX idx_title_search_terms_title_kind_language
ON title_search_terms (title_id, term_kind, literal_term, COALESCE(language_tag, ''));
UPDATE title_search_meta SET collation_version = '' WHERE id = 1;
