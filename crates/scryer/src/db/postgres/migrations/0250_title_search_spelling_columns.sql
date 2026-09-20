-- PostgreSQL half of the multilingual title-search projection. See the SQLite
-- migration of the same number for why these columns exist and why the
-- collation keys carry a version stamp.

ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS literal_term TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS stripped_year_key TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS match_term TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS match_year BIGINT;
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS script TEXT NOT NULL DEFAULT 'other';
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS numbers_key TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS char_length BIGINT NOT NULL DEFAULT 0;
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS romanization_key TEXT;
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS language_tag TEXT;
ALTER TABLE title_search_terms ADD COLUMN IF NOT EXISTS title_year BIGINT;

CREATE TABLE IF NOT EXISTS title_search_collation_keys (
    term_id BIGINT NOT NULL REFERENCES title_search_terms(term_id) ON DELETE CASCADE,
    profile TEXT NOT NULL,
    collation_key BYTEA NOT NULL,
    PRIMARY KEY (term_id, profile)
);

CREATE INDEX IF NOT EXISTS idx_title_search_collation_keys_profile_key
    ON title_search_collation_keys USING btree (profile, collation_key);

CREATE TABLE IF NOT EXISTS title_search_meta (
    id BIGINT NOT NULL PRIMARY KEY,
    collation_version TEXT NOT NULL DEFAULT '',
    projection_generation BIGINT NOT NULL DEFAULT 0,
    CONSTRAINT title_search_meta_single_row CHECK (id = 1)
);

INSERT INTO title_search_meta (id, collation_version, projection_generation)
VALUES (1, '', 0)
ON CONFLICT (id) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_title_search_terms_bucket
    ON title_search_terms USING btree (facet, script, numbers_key, char_length);

CREATE INDEX IF NOT EXISTS idx_title_search_terms_facet_literal
    ON title_search_terms USING btree (facet, literal_term);

CREATE INDEX IF NOT EXISTS idx_title_search_terms_facet_match_term
    ON title_search_terms USING btree (facet, match_term);

CREATE INDEX IF NOT EXISTS idx_title_search_terms_facet_romanization
    ON title_search_terms USING btree (facet, romanization_key);

CREATE INDEX IF NOT EXISTS idx_title_search_terms_stripped_year_key
    ON title_search_terms USING btree (term_kind, stripped_year_key);

-- Same reason as SQLite: the new values need ICU collation and Unicode
-- normalization, so the rows are emptied and the maintainer rebuilds them.
DELETE FROM title_search_terms;
