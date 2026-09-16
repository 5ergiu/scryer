# Scryer 0.21.0 release notes

These notes cover what's changed since **0.20.1**.

## Highlights

- **The `.nfo` files Scryer writes now carry the full picture.** When writing `.nfo` files on import is turned on, Scryer writes the same Kodi-dialect metadata Sonarr and Radarr write, which Jellyfin, Emby, Kodi and Plex all read: one file describing each series, and one beside every imported movie and episode.
  - Sidecars now include titles and sort titles, plot, ratings from each source, release and air dates, runtime, genres, tags, studio, network, country, status, artwork, cast and crew, every external id the title has, and the video, audio and subtitle details of the file that was actually imported — including HDR format, Dolby Vision, resolution, aspect ratio and bitrate.
  - A file that holds more than one episode gets one entry per episode, so both episodes are recognized instead of just the first.
  - Writing is still off by default, and is set separately for movies, series and anime. Each library can override its category's setting to turn writing on or off for that library alone.
  - **An existing `.nfo` file is never replaced**, whatever its size or contents. Files you curate by hand, or that another tool wrote, are left exactly as they are, and Scryer records that it skipped them.
  - A sidecar that can't be written never fails the import; the media file still lands.

- **Scryer asks your download clients and indexers for far less.** Polling was the single largest source of steady background load, and every lane has been cut down.
  - Each poll of a download client now reads its queue once and its history once, whatever the size of either. SABnzbd, NZBGet, qBittorrent and Weaver all behave the same way, reading the most recent 100 completed items per poll and resolving what they belong to from what it already has instead of asking again per item. Large or long-lived queues no longer cost more every tick.
  - Weaver is reconciled in one place instead of two. There is a single background reconciliation for a Weaver connection, it runs every 60 seconds by default, and it no longer makes a separate request per completed download.
  - RSS now runs on its own five-minute rhythm, and each cycle first asks which indexers are actually due for a poll. A cycle with nothing due and nothing waiting to be re-checked does no work at all, so most cycles cost nothing. Feeds are still polled on the cadence you configured, and a feed at risk of moving on before its next poll is still brought forward.
  - Two environment variables let you tune this: `SCRYER_RSS_SYNC_TICK_SECS` sets how often the RSS cycle runs (default 300, minimum 5; a value below your configured RSS cadence does not poll a feed more often than the cadence allows), and `SCRYER_WEAVER_BRIDGE_RECONCILE_INTERVAL_SECS` sets the Weaver reconciliation interval (default 60, and 0 turns it off).

- **Subtitle searching is now a permission you grant per library.** Libraries have a new **Manage Subtitles** permission that covers searching for a subtitle, downloading one, deleting one, and adding one to the blocklist.
  - **Manage Titles includes it**, so anyone who already manages a library's titles keeps doing everything they did before, and the permission is shown as included rather than as a separate box to tick.
  - Users who can only view a library can no longer search for or download subtitles. They still see which subtitles a file has; the buttons that change them are gone.
  - The subtitle search window now says why it cannot search — no provider configured, the provider unavailable, or subtitle downloading turned off — instead of leaving the search button dead.
  - Subtitle providers and their credentials stay administrator-only: granting Manage Subtitles never exposes provider settings, and the link to them appears only for administrators who can open catalog settings.
  - Administrators of the whole catalog keep managing subtitles in every library.
- **Pages scroll again on Android phones.** The catalog list, title overviews and the settings side menu no longer trap a swipe on Android Chrome, so the page moves with your finger as it already did on iOS.

## Included fixes

- **Rename:** external subtitles now follow their video file in Scryer as well as on disk. A rename moved the subtitle files, but the title kept listing them at their old location. Downloaded subtitles keep their provider, score and sync details after the move. The title page still doesn't refresh on its own after a rename; reopen the title to see the new paths. This addresses issue #226.
- **Permissions:** users who can only view a library no longer see **Delete file** or **Make primary** on a movie's files. Scryer already refused both actions for them; the buttons are now hidden too.
- **Subtitle providers:** saving a subtitle provider now tests its connection first and only saves when the test passes, as saving a download client does. A new provider also keeps the content types its plugin recommends (Anime for Jimaku). Before, a provider added without changing the preselected type was saved with no content types, and Scryer never searched it. Saving now requires at least one content type. If a provider's **Content types** column shows `-`, edit it, tick the types it should cover, and save.
- **Subtitles:** **Delete** and **Blocklist** in the subtitle search window now open their confirmation on top of the window. Before, the confirmation opened hidden behind it, so both buttons seemed to do nothing.
- **Keyboard:** Escape closes an open movie overview, as it already did for series and anime.
- **Notifications:** warning and info notifications have a solid background like success and error notifications, so the page behind them no longer shows through.

## API changes

These affect scripts and tools that call Scryer's GraphQL API. The Scryer web app is already updated.

- **Breaking:** the `searchSubtitles` mutation now returns a `SubtitleSearchPayload` object instead of a list of results. The matches are in its `results` field. The payload also carries:
  - `status`: `READY`, `NO_PROVIDERS`, `PROVIDER_UNAVAILABLE` or `DISABLED`. It says why the search returned what it did.
  - `language`: the language that was actually searched.
  - `availableLanguages`: the subtitle languages an administrator configured.

  Change a query that selected result fields directly on `searchSubtitles` so it selects them under `results { ... }`.
- **Added:** the library permission enum `LibraryPermissionValue` has a new value, `MANAGE_SUBTITLES`. Clients that match every enum value should handle it.

## Upgrading

No database or configuration changes are required.

`SCRYER_DOWNLOAD_QUEUE_RECONCILE_MAX_AGE_HOURS` is no longer read. If you set it, you can remove it; leaving it in place has no effect.
