# Scryer 0.21.11 release notes

These notes cover what's changed since **0.21.10**.

## Highlights

- **Lists (Experimental): follow lists and bring their titles into Scryer.** Lists is an experimental feature in this release. The new **Lists** page lets you browse providers, follow a list by URL, preview what it would add, and choose libraries, quality profiles, tags and filters. Lists can add monitored titles, search immediately, or hold requests for review. You can inspect membership and sync history, sync on demand, and exclude titles you do not want added again. Administrators can set members' personal-list policies. Lists requires **Experimental Features** and the **Manage Lists** permission; available sources depend on the provider. When a title leaves a list, the configured action can keep it, record the departure, unmonitor it or tag it. Lists never delete the title or its files.

- **Search an entire season from its title page.** Each season now has an interactive search button, without needing to select an individual episode. Grabs use the result's episode or pack scope, so a season search can queue a pack correctly. The separate **Indexer Search** page is now available without Experimental Features and is labelled **Beta**.

- **Filter your catalog by what is on disk.** New **Missing**, **Partial** and **Complete** filters show file coverage, with counts alongside them. For series, completeness considers monitored episodes that have aired, so future episodes do not keep a caught-up show marked partial. **Needs attention** finds titles with live primary files whose scan failed or needs review.

- **Rego editors understand Scryer's rule inputs.** Scoring, request and maintenance rule editors now offer field completions, helper signatures, documentation and snippets appropriate to that rule family. Use automatic suggestions or **Ctrl-Space**. Editable rules validate after a short typing pause, with inline diagnostics where an error location is available. Explicit validation remains available.

- **Rule scoring retains the facts from the original release listing.** New grabs preserve listing age, votes, password protection, reported languages and extra indexer attributes. Import checks and later scoring of the stored file use those saved facts, including its age at grab time. The media **Info** dialog shows the saved listing, and the rule tester can score a stored file or accept listing facts for a sample release. Search results Scryer reuses instead of asking the indexer again keep the same facts, so a release scores the same either way. This addresses issue #250, where rules reading `input.release.age_days` and the other listing inputs never matched.

- **File logs rotate while Scryer is running.** File logging now rotates on a UTC day change or after reaching the size limit, with gzip archives compressed in the background. Defaults remain **10 MiB** and **five archives**. Routine acquisition, worker and metadata messages move from INFO to DEBUG, while grabs, import outcomes, warnings and errors remain visible.

## Included fixes

- **Anime matching:** absolute episode matching uses a consistent numbering scale for each title. Explicit season and episode numbers are no longer overridden by franchise-name matches or audio-channel tokens such as `2.0`, `5.1` and `7.1`. Cour-local numbering, localized titles and pack companions receive additional handling.
- **Anime searching:** automatic searches keep the query that uses the name of the cour an episode belongs to, send the AniDB id of that cour, and keep both numbering forms of an anime id query.
- **Lists:** a list you follow while another list sync is running is now synced as soon as that run ends instead of waiting for the next scheduled pass; the same applies to **Sync now** and **Sync all**. A scheduled pass now syncs every list that is due, not only the first batch. The action you chose for titles that leave a list runs once the last list holding the title drops it. Adding or removing an exclusion takes effect on the next sync. A new follow starts with a limit of 25 additions per sync. Users with **Manage Lists** can read quality profiles to route a list.
- **Lists:** deleting a user now warns that the lists they follow are deleted with them.
- **Lists:** a **Hold** list parks every item for review, whoever owns the list. An owner with **Manage Titles** but not **Request** can see those held requests under **My requests** and change or cancel them. A **Request** list owned by someone who may manage titles in the routed library adds the title and searches for it instead of raising a request only they could approve. A title that leaves a list no longer stays on the list's page. An item Scryer refuses to add, because it is invalid or no longer exists, is settled as declined instead of being retried on every sync; it is tried again when the item or the list's settings change.
- **Series metadata:** series whose primary metadata identity is TMDB can now be added, previewed, hydrated and searched without inventing a TVDB ID. Episode metadata retains TMDB IDs where TVDB IDs are unavailable.
- **Release parsing:** recognize Ukrainian language tags and title spelling, improve Japanese romanized-title matching, and keep `HC` after a language tag from being misread.
- **Downloads:** reconcile tracked downloads using their canonical identity, avoid treating a download's own binding as a conflicting title assignment, and allow assigned grabs to queue additional files. Deleting a title clears its tracked download associations.
- **Pending releases:** keep saved packs on standby while an already queued release covers their episodes, avoiding redundant acquisition attempts.
- **Downloads:** a grab is recorded before it is sent to the download client, and is not sent if it cannot be recorded. A download that finishes before the client has answered is still matched to its grab and imported for the purpose it was grabbed for. A grab the client refuses leaves nothing behind. Ignoring a download, or a download that disappears from its client, no longer blocks a new grab for the same title for up to 30 seconds afterwards.
- **Background searching:** checking download clients for failed downloads and searching the library now run on separate schedules. Failed downloads are still checked every minute; the library search walk runs every five minutes by default. Adding a title, editing a quality profile, a failed download and similar changes still start a search right away. Set the new **Search walk interval** under acquisition settings; it applies without a restart. On a library with more missing or upgradable items than one batch holds, scheduled backfill is slower than before unless you lower the interval.
- **Background searching:** a batch no longer ends early when the items it picked turn out to need no search; it carries on to the next items. Searches you start yourself are paced as interactive searches instead of at the slower background pace.
- **Import rules:** a runtime error in a post-download rule now holds the import for review instead of allowing it through on a partial score. The message identifies the failing rule, and retrying evaluates the current rules again. The hold does not move, recycle or blocklist the file.
- **Pending imports:** the candidates offered for a pending import are marked as already in your library using the same ids the matcher uses, and a movie is matched by its TMDB id.
- **Title merge:** merging two titles now hands the merged title's downloads and pending cleanup to the title that remains, instead of abandoning them. Nothing is removed from the download client.
- **Renaming:** imports and library renames render the same filename tokens, and series and anime filenames preserve the year in the title. Two new tokens, `title_with_year` and `title_without_year`, let a custom template print the year exactly once.
- **Recycle bin:** restoring an upgrade-recycled file preserves its saved acquisition metadata and score, records a **File Restored** event in the title's history, and refreshes title history. Recycle location and retention settings are grouped under Advanced, with a folder picker for the location.
- **Recycle bin:** changing the recycle bin location moves the existing entries to the new location. Each entry is verified before it is removed from the old location; an entry that cannot be moved stays where it was and is reported on the settings page.
- **Recycle bin:** a custom recycle bin folder must sit outside every library folder, not only the folders of the library being worked on. If it sits inside one, or contains one, Scryer refuses to recycle into it or purge from it, leaves the files where they are, and reports the problem under Health. Adding or changing a library folder that would collide with the custom recycle bin is refused. The default recycle bin inside each library folder is unaffected.
- **Indexers:** an optional per-indexer query budget limits requests per minute. Recovering indexers are paced more cautiously after rate limiting. Expired interactive search results and their grab dialogs are cleared instead of leaving unusable results on screen.
- **Indexers:** the query budget, the configured interval and the slower background pace now apply to the built-in Newznab and Torznab indexers as well; they used to be skipped for these. Automatic searches on a paced indexer take longer as a result. An interactive search skips an indexer whose next slot is more than 30 seconds away and shows how long until it is available.
- **Metadata:** when the metadata service returns an error, Scryer now reports that error and retries on its normal schedule instead of quietly trying an older kind of request.
- **Logs:** the active log file is rotated once it reaches four times the size limit even while an earlier archive is still being compressed.
- **Logs:** a log compression or cleanup job that cannot succeed waits for the next rotation or a restart instead of retrying every minute.
- **Rules and profiles:** the source-video template honors **Allow x265 at non-4K**. Rule tests and interactive title searches retain the title's tags as scoring context.
- **Live updates:** requests, job runs, title history and episode file views refresh as their underlying state changes. Full-hash backfill failures appear in job details, and automatically approved requests identify the permission that approved them.
- **Lists and requests:** mutation inputs no longer carry client cache metadata back to GraphQL, fixing rejected list edits, request submissions and monitor-selection updates.
- **Discover:** presentation refreshes follow metadata-language changes and ignore superseded runs; theme admission and affinity evidence are taken from the metadata service.
- **Notifications and privacy:** grab notifications include more release details, import-complete notifications identify upgrades, and title deletion and move events carry file paths. A manual movie import sends one **Import Complete** notification instead of two, and an upgrade of a movie that belongs to a series now sends one. Indexer and tracker credentials are redacted from stored hints and exposed release data. Failed logins are logged.
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
- **Acquisition settings:** `walkIntervalSeconds` sets the search walk interval. It is always returned; on update, omitting it keeps the stored value. `pollIntervalSeconds` now controls only the failed-download check.
- **Lists:** `ListMembershipPayload.leftAt` is removed; list membership pages return only titles still on the list.
- **Jobs and recycling:** `latestJobRuns` exposes recent runs, and recycle settings expose the configured path, retention, effective paths and validation errors. Saving a changed recycle bin location reports the entries it moved and those it could not in `RecycleBinRelocationPayload`. List activity adds notification event values.
- **Deprecated acquisition settings:** `upgradeCooldownHours`, `crossTierMinDelta` and `forcedUpgradeDeltaBypass` were unused and have been removed from the settings page. Their GraphQL inputs remain accepted but ignored, and outputs report zero. The same-tier minimum score delta remains active.

## Upgrading

Scryer applies database migrations on startup for query budgets, lists, episode numbering, saved release listings, discovery evidence and TMDB episode IDs, on both SQLite and PostgreSQL.

Existing media files without a saved release listing keep unknown listing facts; the upgrade cannot reconstruct the original indexer response.

If you set a custom recycle bin folder inside a library folder, recycling stops until you move it outside every library folder; check Health after upgrading. Files that would have been recycled are left in place.

To change file-log limits, set `SCRYER_LOG_MAX_SIZE` to a positive integer number of bytes and `SCRYER_LOG_MAX_FILES` to a positive integer archive count. Invalid explicit values fail startup when file logging is enabled. Rotation applies to configured file logs and the Windows default file log. Legacy numbered archives are preserved; retention applies to the new archive namespace. Docker stdout retention remains Docker's responsibility.

Backups must be restored with the same Scryer version that created them. Create a fresh backup after upgrading for use with this version.
