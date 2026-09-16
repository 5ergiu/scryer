# Scryer 0.20.2 release notes

These notes cover what's changed since **0.20.1**.

## Highlights

- **The `.nfo` files Scryer writes now carry the full picture.** When writing `.nfo` files on import is turned on, Scryer writes the same Kodi-dialect metadata Sonarr and Radarr write, which Jellyfin, Emby, Kodi and Plex all read: one file describing each series, and one beside every imported movie and episode.
  - Sidecars now include titles and sort titles, plot, ratings from each source, release and air dates, runtime, genres, tags, studio, network, country, status, artwork, cast and crew, every external id the title has, and the video, audio and subtitle details of the file that was actually imported — including HDR format, Dolby Vision, resolution, aspect ratio and bitrate.
  - A file that holds more than one episode gets one entry per episode, so both episodes are recognized instead of just the first.
  - Writing is still off by default, and is set separately for movies, series and anime. Each library can override its category's setting to turn writing on or off for that library alone.
  - **An existing `.nfo` file is never replaced**, whatever its size or contents. Files you curate by hand, or that another tool wrote, are left exactly as they are, and Scryer records that it skipped them.
  - A sidecar that can't be written never fails the import; the media file still lands.

## Upgrading

No database or configuration changes are required.
