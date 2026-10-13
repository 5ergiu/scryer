# Scryer 0.21.15 release notes

These notes cover what's changed since **0.21.14**.

## Highlights

- **Lists is now beta**, available without the experimental feature switch. Personal accounts, provider linking, filtering, previews, and sync reporting have been expanded and hardened.
- **Custom jobs** bring scheduled scripts to the Jobs page, with manual, interval, daily, weekly, and cron schedules, plus an option to run at startup.
- **Localized release search** uses selected search languages and localized title aliases, with per-language learning and paged searches for indexer plugins that support them.
- **Import and cleanup fixes** improve archive handling, held-source recovery, PostgreSQL compatibility, additional-file selection, and download-client state handling.

## Lists beta

- Link multiple accounts per provider and see which account owns each list. Personal accounts and subscriptions remain private to their owner.
- Provider linking supports member-supplied API keys and custom OAuth applications where applicable. TMDb linking waits for approval and uses the member's key; Trakt linking supports PKCE.
- Linking and renewal distinguish temporary service failures and rate limits from rejected credentials. Cancellation and failed persistence clean up grants appropriately, while pending approvals remain visible.
- List filters use canonical genres, ratings, and facet-specific options. Anime filtering includes adult-content exclusion; specials, filler, and recap controls show inherited library values. Movie identities are kept separate from series routing.
- Provider tables support sorting and previews. Hover cards show artwork, ratings, genres, and descriptions, and list items prefer canonical or library titles over provider aliases. Exclusions can be searched by title and retain the correct external-ID type.
- List changes update live, completed syncs produce notifications, and sync history covers one week with the newest 20 runs shown initially. Error and coverage reporting is clearer.
- PostgreSQL fixes cover list caches, memberships, vocabulary, and movie-target bindings.

## Custom jobs and scripts

- Custom jobs appear above system jobs on the Jobs page. The shared script editor supports Shell, Python, PowerShell, Batch, and Go, along with script files that use a shebang.
- Configure interpreter executable paths under **Settings > Scripts**. Interpreters must be available on the Scryer host or in its container. Choose script files through the server file browser.
- Jobs support blocking or fire-and-forget execution and record output, exit status, and duration. Managing scheduled scripts requires the system-settings permission.
- Schedule validation and execution handle host-local daily, weekly, and cron times, daylight-saving transitions, configuration reloads, and shutdown cancellation. Saved interval values are preserved without truncation to whole minutes.
- Post-import scripts remain under **Automation > Post-Processing**. Existing scripts retain their post-import trigger and Shell language; upgrading does not turn them into scheduled jobs.

## Search and acquisition

- Localized alias searches retain ID-based provider coverage. Learning is bounded per title and tracked by language.
- Supporting indexer plugins can return search cursors. Background searches follow and resume pages, preserve overflow at page limits, stop repeated cursors, and back off capped strategies.
- NZBs compressed with gzip, zlib, or zstd are decoded before staging, with corrected zlib-header detection.
- Stored search results do not replay password-hinted candidates. Release parsing recognizes leading bracketed groups in any script and improves alternate-cut classification without treating theatrical or framing labels as alternate cuts.
- Upgrade comparisons use current rules for existing files, including files awaiting review. Automatic grabs retain their attribution, and standby grabs are not recorded twice.
- Credentials are scrubbed from stored URLs, release hints, and failure/blocklist history; a migration also sanitizes historical URLs.

## Imports, media, and library safety

- Archive handling preserves password candidates across retries and improves nested scene-archive and subtitle processing. Malformed nested subtitle archives no longer fail the entire subtitle download, and filesystem quota exhaustion holds extraction for recovery.
- Held-source release handles pending subtitles and related imports and locators. Unreadable or incomplete evidence keeps sources and workspaces held rather than removing files whose disposition cannot be established.
- PostgreSQL JSON decoding is fixed for cleanup and import hold, release, and retry paths, including download-history cleanup and torrent seeding reconciliation.
- NZBGet and SABnzbd handling better distinguishes paused, repairing, warning, duplicate, and terminal jobs. Cleanup respects ownership and mapped job state, and failures to prune empty directories no longer unnecessarily fail completed work. NZBGet's warning timeout is restored to 24 hours.
- Executable-only unwanted downloads fail explicitly. Additional episode files can auto-promote only when exactly one eligible additional file remains and no primary exists. Movie and series-movie additional files and alternate cuts are not automatically promoted; scans preserve that distinction and wanted state.
- Cropped-video resolution detection is corrected. Existing metadata is relabeled from stored dimensions, including cropped 1080p and 2160p files, without moving or re-encoding media.
- Copy verification now defaults to **Quick**, which samples the beginning and end of a file. **Full** verification remains available, saved Full selections are preserved, and location operations continue to use Full verification.
- Folder safeguards introduced in 0.21.14 are extended with canonical-path and same-library boundary checks. Single-title renames check overlaps with other titles, folder failures are reported correctly, and multi-episode files are counted once.
- Metadata refresh respects upstream episode information and skip policies. Untitled episodes display **TBA** without using it as a search title; calendar episode cards can fall back to the title backdrop.

## Settings, requests, and reliability

- Interface-language changes synchronize across browser tabs. Layout and column preferences are stored per device class, with serialized saves to prevent late requests overwriting newer choices.
- Requests can be filtered by requester, and dismissed requests can be reopened. History distinguishes rule-rejected imports and preserves import/source context for upgraded media.
- Public URL configuration is unified. Administrators can edit it unless `SCRYER_PUBLIC_URL` overrides the saved setting; account linking uses changes immediately. Passkey impact is previewed before potentially disruptive changes. Passkey relying-party configuration is derived at startup when explicit overrides are absent, so changes affecting it require a restart.
- PostgreSQL startup uses bounded connection retries. Filesystem and log work is moved off asynchronous workers, cleanup candidate selection is incremental, and Windows allocator decommit purging is restored.

## Bugfixes

### Imports, archives, and subtitles

- Imports fall back to a verified copy when a filesystem reports a successful hard link but cannot prove that the link shares the source's identity. This covers mounts with unstable inode identities, including some FUSE and CIFS configurations. The affected volume is remembered until restart so subsequent imports avoid repeating the failed link attempt; a changed source still fails verification.
- Files produced in an owned extraction workspace can be placed by a no-replace rename in every import mode. Later failure handling does not move the placed file back or delete it as though it were still extraction output.
- Archive-password candidates survive submission, extraction, and retry. Password failures expose a dedicated retry action, rather than requiring the operator to start the entire acquisition again.
- Nested archive and sidecar handling preserves source ownership and verification evidence across manual, episode, and series-movie imports. Subtitle delivery is accounted for before source cleanup becomes eligible.
- Archive-plugin filesystem limits are enforced during extraction. Quota exhaustion leaves the extraction held for recovery, and malformed nested subtitle archives are skipped without discarding the rest of the subtitle download.
- Releasing a held download considers every associated import and source locator, including sources held for pending subtitles. A missing record or unreadable verification evidence leaves the operation resumable rather than falsely completing it.
- Workspace cleanup checks ownership and release evidence for the specific workspace. Unproven workspaces and held sources remain protected, including after a partial release or recovery attempt.
- An owned download containing only unwanted executables is reported as failed instead of remaining without a meaningful import result.
- Post-download upgrade decisions re-score the existing file using current rules, so a stale stored score cannot decide whether a replacement is accepted.
- Import-preservation payloads decode correctly from PostgreSQL native JSON as well as SQLite text. The fix covers cleanup, held-source release, import retries, and seeding reconciliation.

### Download clients and cleanup

- SABnzbd cleanup validates category isolation, sorting, and history retention, honors data-preservation choices, and reports unsuccessful queue removal instead of assuming it succeeded.
- SABnzbd cleanup recognizes sanitized download names, imported series artifacts, and empty job-parent folders while retaining files whose ownership or completed import cannot be verified.
- SABnzbd queue pause applies to downloading jobs without incorrectly overriding active repair or other post-processing states. History warnings, duplicate jobs, and missing history status receive the appropriate state or explanation.
- SABnzbd history archival uses the mapped job state, keeping failed-job removal distinct from completed-job cleanup. An empty-folder pruning failure does not undo an otherwise completed operation.
- NZBGet queue deletion and terminal history cleanup use the appropriate client operations and admission checks. Warning jobs retain their 24-hour timeout, and cleanup avoids unnecessary repeated RPCs and logs.
- Cleanup can start from downloads changed since the previous pass, while retaining periodic full passes so older eligible work is not forgotten.

### Library scans, renames, and media identity

- Primary-file removal promotes an additional episode file only when exactly one candidate remains and no primary exists at write time, including bulk removal. The promoted file retains episode search coverage.
- Movie and series-movie additional files remain additional until explicitly promoted. A scan does not elect an alternate cut or a losing file simply because it relinks a stored record.
- Movies with only an alternate cut, and series-movie episodes with only a losing file, remain wanted. A series-movie file stored before its episode association can still be elected once that association is established.
- Missing and cutoff searches count a file awaiting review as an existing file in its scope rather than treating the scope as empty.
- A scan refuses to walk a movie record whose folder spans a library root. Folder claims must remain inside the title's own library, with `.` and `..` resolved before boundary checks.
- Single-title renames check destinations against other titles' folders. Rename failures to record a folder are reported as failures, and multi-episode files contribute only once to folder accounting.
- A rename whose file operations are all no-ops can still repair a title incorrectly recorded at the library root.
- Cropped-resolution classification and database ranking use the same tolerant ladder. Existing bare resolution labels are corrected from stored dimensions while compound and interlaced labels are preserved.
- Episode metadata refresh follows upstream changes and configured skip policies. Untitled episodes retain episode-number history labels, and the displayed TBA label is excluded from import searches.

### Search, parsing, and acquisition history

- Raw Indexers-page queries bypass title-search strategies: no movie categories are injected, only requested categories are sent, and results are not rejected for failing to match the query as a catalog title.
- Raw searches report why an indexer was skipped, deduplicate exact releases, and count each indexer's contribution after deduplication. The raw-query page size is separate from title-search result limits.
- Localized searches preserve ID-only indexer coverage while trying aliases. Search-language choices are validated against the supported picker set, and query learning is keyed by language and bounded per title.
- Title mutations reject reserved search-option tags in the raw tags field, preventing that field from bypassing the dedicated search-option handling.
- Resumed indexer pages retain results beyond the current limit, repeated cursors terminate safely, and capped strategies receive their own backoff.
- Gzip, zlib, and zstd NZBs are inflated before staging; zlib detection accepts valid RFC 1950 headers rather than relying on a single header value.
- Password-hinted releases are not replayed from stored candidates. Indexer and tracker credentials are removed from persisted release URLs, grab hints, and failed or blocklisted download events, including historical records.
- Leading bracketed release groups are accepted in non-Latin scripts. Fan edits, assembly cuts, alternate endings, and other alternate-cut editions are recognized without misclassifying theatrical releases, framing labels, or the abbreviation DC.
- Automatic-search grabs record the actor who initiated them. Standby grabs are recorded once, and media-upgrade events retain their import ID and source reference.

### Lists, provider accounts, and synchronization

- Plex uses a stable client identifier per instance. Host-issued OAuth client IDs survive plugin configuration, and Trakt PKCE exchanges omit an unrelated stored client secret.
- TMDb approval can remain pending without failing the link. A temporary identity lookup failure after approval does not discard the approved account link.
- Transient polling failures keep the link pending and visible. Definitive refusals terminate it, expired grants are discarded, and replay of a completed link is bounded.
- Abandoned account links are cancelled on the instance. A grant is revoked if its account link cannot be persisted.
- Credential renewal finishes even if its list sync is cancelled. A renewed credential is retained through a failed save, and local storage failures or SMG errors are not misclassified as provider credential refusals.
- Provider rate limits retain their requested pause. Simkl renewal accepts a shortened refresh deadline, and provider refusal messages use structured reasons rather than parsing display text.
- Provider account catalogs are not fetched twice immediately after linking or reopening. Failure to read one account's lists is shown beside that account.
- List items leaving a provider remain pending when their route cannot yet be resolved. Departures the owner lacks permission to act on are recorded appropriately without repeatedly forcing provider reads.
- List route options carry through both direct additions and requests. Anime chart entries reported as series can still match the anime route, while movie identity remains distinct from series routing.
- An unchanged provider response does not bypass metadata-dependent filtering or unfinished work. Unresolved entries recover the provider's original item kind before routing instead of trusting a fallback stored kind.
- Partition fingerprints survive provider responses that omit them. Native JSON reads and writes work for cached list data, memberships, vocabulary, and movie targets on PostgreSQL.
- Preview limits are honored, and oversized metadata queries use POST rather than exceeding URL limits.
- Switching providers resets the previous catalog's expanded previews. Preview artwork uses appropriate poster sizes, title flyouts appear above dialogs, and language labels use full names.
- Re-enabling a list does not misrepresent its sync status. Finished syncs and subscription changes reach all open Lists surfaces through the live feed.
- Exclusions retain their external-ID kinds, inherited episode-handling controls show their effective library values, and filtered entries explain the reason in readable terms.
- Account and request failures are localized using structured error reasons. API-key and secret fields discourage password-manager autofill, and account-discovered source parameters are hidden from manual entry.
- List rows, previews, coverage colors, type pills, and action controls use consistent layouts. Library entries link to their titles, with library or resolved metadata names taking precedence over provider aliases.

### Custom jobs and script execution

- Saving a disabled custom job creates it disabled immediately, without a brief enabled interval. Editing a script does not change its creation-time trigger.
- Interpreter settings accept partial updates and validate supplied paths. Interpreter selection is resolved once per run, and the script root is made absolute so execution does not depend on the working directory.
- Materialized scripts run through their selected interpreter, and output collection is bounded so inherited output handles cannot keep a completed run waiting indefinitely.
- Changing an inline script's language requires acknowledgment. File-backed scripts require a selected file, and interpreter paths have file pickers.
- Scheduled-script execution and run-history access enforce the appropriate trigger permission.
- Schedule times are stored in normalized `HH:MM` form. Validation handles daylight-saving transitions, and editing preserves intervals that are not an exact number of minutes.
- Initial schedule loading retries after failure; reloads are serialized and retried. Long waits are recalculated from the wall clock so clock changes do not leave stale scheduling delays.
- Running scheduled scripts are terminated at shutdown. Fire-and-forget runs left marked running are reconciled at startup.
- Run-history retention is applied per custom job, rather than letting one job's runs displace another's history. Latest-run lookup also distinguishes individual custom jobs.
- Job history joins each run to its own captured script output and refreshes output that arrives after a fire-and-forget job finishes. Stale history responses cannot overwrite a newer response.
- Weekly and cron schedules are represented correctly on the Jobs page, and cron descriptions load the selected locale. Script-editor labels, pickers, and schedule fields use consistent controls and spacing.

### Interface, settings, and runtime

- Late language selections are saved to the profile, superseded selections are discarded, and browser tabs stay synchronized. Concurrent profile and catalog-layout saves are serialized to preserve the latest choices.
- The Requests view keeps its layout stable when switching modes and tabs. Calendar episode flyouts use the title background when an episode still is unavailable.
- Status notifications use an explicit severity rather than inferring it from message wording. Rule-pack actions and confirmation buttons use consistent controls.
- Public-URL changes show their passkey impact and require acknowledgment when they could disrupt sign-in. Account linking reads the effective URL without requiring a restart, while passkey configuration retains its startup behavior.
- Installed plugin settings are isolated per plugin. List-provider plugin panels use a responsive drawer, and plugin cards reuse provider artwork.
- PostgreSQL startup tolerates temporary connection failures within a bounded window. Added indexes cover child foreign keys, title lookup keys, download ordering, discovery generations, and custom-job history.
- Slow database statements above two seconds are logged at debug level. Import filesystem checks and log sinks are isolated from asynchronous runtime workers, and grab-stage logging is reduced to debug.
- Windows builds restore allocator decommit purging.

## Upgrading and API compatibility

- **IMDb public user lists are no longer supported.** Upgrading removes their subscriptions and associated routes, memberships, list-scoped exclusions, and sync history. Existing titles, requests, and media files are preserved. Existing IMDb chart subscriptions are retained.
- API integrations must stop sending **`SearchReleasesInput.kind`**; that field and `InteractiveSearchKindValue` have been removed. Search inputs now select their subject through `titleId` or `query`.
- The new Lists, script, and configuration API fields use typed enums. These fields were introduced after 0.21.14, so clients built against intermediate 0.21.15 development snapshots should regenerate their schema bindings. List-preview release dates use the `Date` scalar and remain `YYYY-MM-DD` on the wire.
- Review the new Quick copy-verification default and any public-URL/passkey impact when configuring this version. Existing post-import scripts and saved Full verification choices are preserved.
