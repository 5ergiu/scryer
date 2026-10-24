-- An operator who reassigns a file to different episodes pins those links.
-- Library scans, metadata refreshes and re-link passes leave a pinned link
-- alone. The pin lives on the link row, so it goes away with the link: when
-- the file or the episode is deleted, or when the file is reassigned again.
ALTER TABLE file_episode_map ADD COLUMN operator_pinned INTEGER NOT NULL DEFAULT 0;
