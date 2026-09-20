-- One multilingual normalizer for both consumers.
--
-- `title_search_terms` carried only the diacritic-stripped UI form. Release and
-- import resolution key on the diacritic-preserving lookup form, the match form
-- it is compared in (`match_term`, the lookup form minus a name's own trailing
-- year, with `match_year` the year that name then asserts), its script,
-- its numbers guard and its collation keys, all of which lived only in an
-- in-memory index rebuilt per process. These columns persist them so both
-- consumers read one projection.
--
-- `collation_key` blobs are ICU sort keys, which are only comparable within one
-- collation-data version. `title_search_meta.collation_version` stamps the
-- fingerprint the rows were written with; the projection maintainer rebuilds
-- whenever the running build's fingerprint differs, which is what makes
-- persisting them sound.

ALTER TABLE title_search_terms ADD COLUMN literal_term TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN stripped_year_key TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN match_term TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN match_year INTEGER;
ALTER TABLE title_search_terms ADD COLUMN script TEXT NOT NULL DEFAULT 'other';
ALTER TABLE title_search_terms ADD COLUMN numbers_key TEXT NOT NULL DEFAULT '';
ALTER TABLE title_search_terms ADD COLUMN char_length INTEGER NOT NULL DEFAULT 0;
ALTER TABLE title_search_terms ADD COLUMN romanization_key TEXT;
ALTER TABLE title_search_terms ADD COLUMN language_tag TEXT;
ALTER TABLE title_search_terms ADD COLUMN title_year INTEGER;

CREATE TABLE IF NOT EXISTS title_search_collation_keys (
    term_id INTEGER NOT NULL REFERENCES title_search_terms(term_id) ON DELETE CASCADE,
    profile TEXT NOT NULL,
    collation_key BLOB NOT NULL,
    PRIMARY KEY (term_id, profile)
);

CREATE INDEX IF NOT EXISTS idx_title_search_collation_keys_profile_key
    ON title_search_collation_keys(profile, collation_key);

CREATE TABLE IF NOT EXISTS title_search_meta (
    id INTEGER NOT NULL PRIMARY KEY CHECK (id = 1),
    collation_version TEXT NOT NULL DEFAULT '',
    projection_generation INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO title_search_meta (id, collation_version, projection_generation)
VALUES (1, '', 0);

-- Bucket scan: the resolver narrows to one facet, script and numbers guard,
-- then to a length band, exactly as the in-memory index's bucket key did.
CREATE INDEX IF NOT EXISTS idx_title_search_terms_bucket
    ON title_search_terms(facet, script, numbers_key, char_length);

CREATE INDEX IF NOT EXISTS idx_title_search_terms_facet_literal
    ON title_search_terms(facet, literal_term);

-- The spelling lane compares the match form, which drops a name's own trailing
-- year; the literal lane above keeps it.
CREATE INDEX IF NOT EXISTS idx_title_search_terms_facet_match_term
    ON title_search_terms(facet, match_term);

CREATE INDEX IF NOT EXISTS idx_title_search_terms_facet_romanization
    ON title_search_terms(facet, romanization_key);

-- Collision counting groups on the year-stripped lookup key.
CREATE INDEX IF NOT EXISTS idx_title_search_terms_stripped_year_key
    ON title_search_terms(term_kind, stripped_year_key);

-- Existing rows predate every column above, and the values cannot be computed
-- in SQL: they need ICU collation and Unicode normalization. Emptying the
-- projection makes the maintainer's "rebuild when empty or stamped with a
-- different collation version" check fire on the next start.
DELETE FROM title_search_spellfix;
DELETE FROM title_search_terms;
