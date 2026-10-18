-- Delay profiles move out of the `acquisition.delay_profiles` JSON setting
-- into their own tables, and gain an optional library scope.
--
-- The scalar fields live on `delay_profiles`. Facets, tags and libraries are
-- each a set, so each gets a link table. An empty set means "every": a profile
-- with no facet rows applies to every facet, one with no tag rows to every
-- title, and one with no library rows to every library.
--
-- Tags stay labels, matching how `titles.tags` holds them, so matching needs
-- no join through `title_tag_definitions`.
--
-- The companion `import_delay_profile_catalog` Rust hook copies the existing
-- JSON catalog into these tables and removes the setting value.
CREATE TABLE delay_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    usenet_delay_minutes INTEGER NOT NULL DEFAULT 0,
    torrent_delay_minutes INTEGER NOT NULL DEFAULT 0,
    preferred_protocol TEXT NOT NULL DEFAULT 'usenet',
    min_age_minutes INTEGER NOT NULL DEFAULT 0,
    bypass_score_threshold INTEGER,
    enable_usenet INTEGER NOT NULL DEFAULT 1,
    enable_torrent INTEGER NOT NULL DEFAULT 1,
    bypass_if_highest_quality INTEGER NOT NULL DEFAULT 0,
    priority INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE delay_profile_facets (
    profile_id TEXT NOT NULL REFERENCES delay_profiles(id) ON DELETE CASCADE,
    facet TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, facet)
);

CREATE TABLE delay_profile_tags (
    profile_id TEXT NOT NULL REFERENCES delay_profiles(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, label)
);

CREATE INDEX idx_delay_profile_tags_label ON delay_profile_tags(label);

-- Deleting a library removes its rows here. The library store disables any
-- profile that loses its last library in the same transaction, so a scoped
-- profile never silently widens to every library.
CREATE TABLE delay_profile_libraries (
    profile_id TEXT NOT NULL REFERENCES delay_profiles(id) ON DELETE CASCADE,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, library_id)
);

CREATE INDEX idx_delay_profile_libraries_library_id
    ON delay_profile_libraries(library_id);
