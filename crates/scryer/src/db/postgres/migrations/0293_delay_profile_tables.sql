-- Delay profile tables. PostgreSQL twin of the SQLite file, which explains
-- the shape; only the column types differ. The `import_delay_profile_catalog`
-- Rust hook in the same migration moves the JSON catalog on both engines.
CREATE TABLE delay_profiles (
    id text PRIMARY KEY NOT NULL,
    name text NOT NULL,
    usenet_delay_minutes bigint NOT NULL DEFAULT 0,
    torrent_delay_minutes bigint NOT NULL DEFAULT 0,
    preferred_protocol text NOT NULL DEFAULT 'usenet',
    min_age_minutes bigint NOT NULL DEFAULT 0,
    bypass_score_threshold integer,
    enable_usenet boolean NOT NULL DEFAULT TRUE,
    enable_torrent boolean NOT NULL DEFAULT TRUE,
    bypass_if_highest_quality boolean NOT NULL DEFAULT FALSE,
    priority integer NOT NULL DEFAULT 0,
    enabled boolean NOT NULL DEFAULT TRUE,
    created_at timestamptz NOT NULL DEFAULT NOW(),
    updated_at timestamptz NOT NULL DEFAULT NOW()
);

CREATE TABLE delay_profile_facets (
    profile_id text NOT NULL REFERENCES delay_profiles(id) ON DELETE CASCADE,
    facet text NOT NULL,
    position integer NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, facet)
);

CREATE TABLE delay_profile_tags (
    profile_id text NOT NULL REFERENCES delay_profiles(id) ON DELETE CASCADE,
    label text NOT NULL,
    position integer NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, label)
);

CREATE INDEX idx_delay_profile_tags_label ON delay_profile_tags(label);

CREATE TABLE delay_profile_libraries (
    profile_id text NOT NULL REFERENCES delay_profiles(id) ON DELETE CASCADE,
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    position integer NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, library_id)
);

CREATE INDEX idx_delay_profile_libraries_library_id
    ON delay_profile_libraries(library_id);
