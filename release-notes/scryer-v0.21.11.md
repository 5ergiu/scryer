# Scryer 0.21.11 release notes

These notes cover what's changed since **0.21.10**.

## Highlights

- **Follow lists and bring their titles into Scryer.** The new **Lists** page lets you browse providers, follow a list by URL, preview what it would add, and choose libraries, quality profiles, tags and filters. Lists can add monitored titles, search immediately, or hold requests for review. You can inspect membership and sync history, sync on demand, and exclude titles you do not want added again. Administrators can set members' personal-list policies. Lists requires **Experimental Features** and the **Manage Lists** permission; available sources depend on the provider. When a title leaves a list, the configured action can keep it, record the departure, unmonitor it or tag it. Lists never delete the title or its files.

- **Search an entire season from its title page.** Each season now has an interactive search button, without needing to select an individual episode. Grabs use the result's episode or pack scope, so a season search can queue a pack correctly. The separate **Indexer Search** page is now available without Experimental Features and is labelled **Beta**.

- **Filter your catalog by what is on disk.** New **Missing**, **Partial** and **Complete** filters show file coverage, with counts alongside them. For series, completeness considers monitored episodes that have aired, so future episodes do not keep a caught-up show marked partial. **Needs attention** finds titles with live primary files whose scan failed or needs review.

- **Rego editors understand Scryer's rule inputs.** Scoring, request and maintenance rule editors now offer field completions, helper signatures, documentation and snippets appropriate to that rule family. Use automatic suggestions or **Ctrl-Space**. Editable rules validate after a short typing pause, with inline diagnostics where an error location is available. Explicit validation remains available.

- **Rule scoring retains the facts from the original release listing.** New grabs preserve listing age, votes, password protection, reported languages and extra indexer attributes. Import checks and later scoring of the stored file use those saved facts, including its age at grab time. The media **Info** dialog shows the saved listing, and the rule tester can score a stored file or accept listing facts for a sample release.

- **File logs rotate while Scryer is running.** File logging now rotates on a UTC day change or after reaching the size limit, with gzip archives compressed in the background. Defaults remain **10 MiB** and **five archives**. Routine acquisition, worker and metadata messages move from INFO to DEBUG, while grabs, import outcomes, warnings and errors remain visible.

## Included fixes

- **Anime matching:** absolute episode matching uses a consistent numbering scale for each title. Explicit season and episode numbers are no longer overridden by franchise-name matches or audio-channel tokens such as `2.0`, `5.1` and `7.1`. Cour-local numbering, localized titles and pack companions receive additional handling.
- **Library scans:** a title scan can correct episode associations previously created by scanning when the filename confidently identifies a different episode set. Ambiguous or incomplete matches are left alone, as are imported and pending-bound files. This corrects database associations without moving or deleting media files.
- **Series metadata:** series whose primary metadata identity is TMDB can now be added, previewed, hydrated and searched without inventing a TVDB ID. Episode metadata retains TMDB IDs where TVDB IDs are unavailable.
- **Release parsing:** recognize Ukrainian language tags and title spelling, improve Japanese romanized-title matching, and keep `HC` after a language tag from being misread.
- **Downloads:** reconcile tracked downloads using their canonical identity, avoid treating a download's own binding as a conflicting title assignment, and allow assigned grabs to queue additional files. Deleting a title clears its tracked download associations.
- **Pending releases:** keep saved packs on standby while an already queued release covers their episodes, avoiding redundant acquisition attempts.
- **Import rules:** a runtime error in a post-download rule now holds the import for review instead of allowing it through on a partial score. The message identifies the failing rule, and retrying evaluates the current rules again. The hold does not move, recycle or blocklist the file.
- **Renaming:** imports and library renames render the same filename tokens, and series and anime filenames preserve the year in the title.
- **Recycle bin:** restoring an upgrade-recycled file preserves its saved acquisition metadata and score, records a **File Restored** event, and refreshes title history. Recycle location and retention settings are grouped under Advanced, with a folder picker for the location.
- **Indexers:** an optional per-indexer query budget limits requests per minute. Recovering indexers are paced more cautiously after rate limiting. Expired interactive search results and their grab dialogs are cleared instead of leaving unusable results on screen.
- **Rules and profiles:** the source-video template honors **Allow x265 at non-4K**. Rule tests and interactive title searches retain the title's tags as scoring context.
- **Live updates:** requests, job runs, title history and episode file views refresh as their underlying state changes. Full-hash backfill failures appear in job details, and automatically approved requests identify the permission that approved them.
- **Lists and requests:** mutation inputs no longer carry client cache metadata back to GraphQL, fixing rejected list edits, request submissions and monitor-selection updates.
- **Discover:** presentation refreshes follow metadata-language changes and ignore superseded runs; theme admission and affinity evidence are taken from the metadata service.
- **Notifications and privacy:** grab notifications include more release details, import-complete notifications identify upgrades, and title deletion and move events carry file paths. Indexer and tracker credentials are redacted from stored hints and exposed release data. Failed logins are logged.
- **Backups:** list accounts, subscriptions, routing, memberships, exclusions, policies and sync history are included. Derived discovery presentation selection is rebuilt after restore.
- **Plugins:** hover over a truncated plugin description to read it in full.
- **Dependencies:** compatible Rust and npm updates retain the existing dependency footprint without adding crates or increasing duplicate package versions.

## API and plugin changes

These affect scripts, integrations and custom rules. The Scryer web app is already updated.

- **Lists:** new GraphQL operations cover providers, previews, subscriptions, memberships, exclusions, member policies and sync history. Titles expose list membership, requests expose their origin, and maintenance rules gain list facts. The plugin SDK adds a list-provider kind and its host interface.
- **Catalog:** `TitleCatalogFilterInput` adds `presences` and `needsAttention`; catalog counts include missing, partial, complete and attention totals.
- **Series metadata:** `MetadataSeriesInput.tvdbId` is now optional and the input accepts `smgId`, `tmdbId` and `imdbId`. Series, season and episode responses expose additional provider IDs. Existing non-null TVDB response fields use an empty string where that identity is unavailable; clients must not assume it is a usable ID.
- **Rule testing:** a test can provide a listing or select `mediaFileId`; `releaseName` is optional when testing a stored file. Media files expose `releaseListing`. Rules receive listing age as `age_days`, anchored to capture time for stored snapshots.
- **Indexer settings:** `maxQueriesPerMinute` configures the query budget. On update, omission preserves it and null clears it. Prowlarr-managed parents do not accept a budget.
- **Jobs and recycling:** `latestJobRuns` exposes recent runs, and recycle settings expose the configured path, retention, effective paths and validation errors. Restores and list activity add notification event values.
- **Deprecated acquisition settings:** `upgradeCooldownHours`, `crossTierMinDelta` and `forcedUpgradeDeltaBypass` were unused and have been removed from the settings page. Their GraphQL inputs remain accepted but ignored, and outputs report zero. The same-tier minimum score delta remains active.

## Upgrading

Scryer applies database migrations on startup for query budgets, lists, episode numbering, saved release listings, discovery evidence and TMDB episode IDs, on both SQLite and PostgreSQL.

Existing media files without a saved release listing keep unknown listing facts; the upgrade cannot reconstruct the original indexer response. Likewise, the scan correction described above is not a blanket repair of imported episode associations.

To change file-log limits, set `SCRYER_LOG_MAX_SIZE` to a positive integer number of bytes and `SCRYER_LOG_MAX_FILES` to a positive integer archive count. Invalid explicit values fail startup when file logging is enabled. Rotation applies to configured file logs and the Windows default file log. Legacy numbered archives are preserved; retention applies to the new archive namespace. Docker stdout retention remains Docker's responsibility.

Backups must be restored with the same Scryer version that created them. Create a fresh backup after upgrading for use with this version.
