# Scryer 0.21.21 release notes

These changes are relative to v0.21.20.

## Highlights

- Add HTTP indexer support and a native HTTP download client for direct media files and subtitle sidecars.
- Show movies in the calendar using their configured availability dates, including market selection, library overrides, delays, and labeled estimates.
- Reassign an imported episode file from the web, and resolve an ambiguous anime import with the same episode picker.
- Add per-user color palettes, a theme and highlight-color quick menu, and a decorative glow preference.
- Spread RSS requests across scheduler ticks, combine each indexer's categories into one poll, and reduce repeated capability and search-index work.
- Change request leases to expiry dates only. Operator keeps and the one-click **Keep** are removed; review the upgrade notes before enabling maintenance rules.

## Upgrade notes

- Maintenance rules and request rules remain experimental and require the experimental features switch.
- A live request lease no longer protects a title from other maintenance rules. Operator keeps, the **Kept** badge, and their controls are removed. Protect titles through rule conditions, such as a tag exclusion, before enabling destructive rules.
- A request approved **forever** or with a zero-day lease no longer creates a lease. **Keep permanently** ends an existing finite lease and leaves the title without an expiry; it is refused when there is no finite lease to end.
- Upgrade removes operator keeps and permanent keep claims, along with non-expiring lease rows that have no positive duration. Finite leases and explicitly assigned expiry dates are preserved.
- The built-in **Expired request leases** rule now matches expired leases without keep-based exemptions. Its grace period, library selection, and storage root are retained. An **Enabled** instance of this rule returns to **Dry run** so its candidates can be reviewed before re-enabling it.
- The `keep_claim_active` maintenance fact is removed. Update custom rules that read it.
- Weaver 0.11.5 or newer is required.
- SQLite applies pending migrations on one connection, fixing upgrades that could fail when a later migration read a schema change through another connection's stale schema cache.

## Movie calendar

- Movies no longer need an episode record to appear in the calendar. Each movie event uses the same effective availability date as automatic acquisition, with the title's threshold taking precedence over the library and Movie facet settings.
- Calendar dates honor the selected release market, theatrical or home-release threshold, configured day offset, and the existing worldwide and theatrical-plus-90-days fallbacks. Legacy theatrical metadata follows the same availability policy.
- Estimated dates are labeled on calendar events and in their hover cards. Unknown dates, pending market refreshes, and announced-only availability do not create an invented date.
- Movie events respect library access and filters, preserve the title's monitoring state, and open the movie overview. Episode calendar behavior is preserved.

## Episode assignment, imports, and libraries

- Add **Reassign** to imported episode files. Choose one or more episodes within the same title, then rename and move the file using the library's naming rules. If a selected episode already has a primary file, explicitly replacing it keeps the old file on disk as an additional file. A failed rename restores the original episode links.
- Reassignment is a one-time match and rename; later scans retain their normal matching behavior. Successful changes appear in title history.
- Pending imports held for ambiguous anime numbering offer the same season and episode picker. The dialog includes the original release name to help identify the intended episode.
- Keep a library root's identity when its path is edited, preserving its title associations and default-root selection.
- Automatic import recognizes extensionless video files by content and gives them the matching extension.
- Preserve the literal anime season when a community interpretation conflicts with a real season unless the alternate reading has supporting evidence. Community searches derive their season from the entry's name or starting TVDB season, avoiding incorrect searches caused by split seasons and incomplete season entries.
- Preserve HTTP download identity through import, so date-only releases use the linked catalog episode's season. Import the subtitle sidecars reported by the HTTP client alongside the media.

## HTTP acquisition and downloads

- Indexer plugins can advertise HTTP releases, including provider-page, direct-file, HLS, and DASH transports. Grabs route to a download client that supports an offered transport. Native HTTP downloads currently use direct-file resources.
- Configure the native HTTP client's output folder, 1 to 8 concurrent downloads, 1 to 8 connections per host, and shared bandwidth limit. It selects a complete media resource within the quality profile's ceiling and includes requested subtitle sidecars.
- Persist the queue and transfer progress under `.scryer-http` in the download folder. Resume interrupted transfers when supported, restart the transfer when the provider declares that resume is unavailable, and recognize files already published before a restart.
- Buffer file writes and checkpoint progress per transfer, rather than rewriting the whole queue. Reuse the egress client while its assigned proxy is unchanged, and apply bandwidth changes to pending transfer waits.
- Honor `Retry-After`, preserve resource headers only on their original origin across redirects, and store response bodies as sent. Request errors omit resource URLs so signed CDN tokens are not exposed through those errors.
- Bound `403` retries and lower-ranked variant fallbacks. Other `4xx` responses fail without retry; a refusal after the resource URL expires asks for a fresh grab.
- Verify provider-supplied SHA-256, SHA-1, or MD5 checksums before publishing a file, and reject media with no video track or an insufficient declared duration. Validation failures preserve the partial file. Media without a usable filename extension receives one from its probed container.
- Optional subtitle failures do not fail the media download. Required subtitle failures identify the language and explain that the media file remains on disk.
- Deleting an HTTP client or changing its type stops its engine. Disabling it allows the current file to finish and holds the remaining queue. These configuration changes preserve its queue and downloaded files.
- Live, upcoming, and removed HTTP releases are refused consistently on submission, with translated reason codes. DRM and sign-in flags are passed to the receiving client rather than automatically refusing the grab. Missing HTTP details require a fresh search; stored HTTP results are never replayed as live search results, and parked releases retain their HTTP description.
- Indexers with plugin-defined endpoints can be configured without a connection URL field, and the hostless HTTP download client supports **Test Connection**.

## RSS, acquisition, and subtitles

- Poll each RSS indexer once with its routed categories merged into one request. The first merged poll resumes from the oldest previous category marker so releases between category checkpoints are not skipped.
- Distribute indexers across the scheduler's tick slots instead of polling them all together. Each tick admits at most 10 due indexers by default; deferred indexers remain due. `SCRYER_RSS_TICK_INDEXER_LIMIT` adjusts this ceiling.
- Use a default RSS request budget of 10 pages and 1,000 items, adjustable through `SCRYER_RSS_POLL_PAGE_BUDGET`, and pass the item budget to the plugin. Preserve additional recent-feed results returned during plugin catch-up within a bounded host safety cap, and warn when the previous marker is not reached.
- Admit one background acquisition pass at a time. Stop its walk at the supervisor's work budget and continue from the unvisited titles on a later cycle instead of overlapping passes or restarting the walk.
- Cache Prowlarr child capabilities for seven days. Refresh missing, expired, or connection-invalidated snapshots instead of requesting every child's capabilities on each sync. Automatic searches wait for an initial capability snapshot, and rate-limited capability requests defer without holding the sync open.
- Coalesce pending fuzzy-index updates and avoid rewriting unchanged searchable projections. Compare stored collation keys independently of database text collation.
- Keep unconfirmed grabs while the client's history does not reach far enough back to prove their absence. Client-history coverage no longer releases an accepted grab and causes it to be downloaded again.
- Skip download-client feedback requests during failure backoff. Subtitle providers that fail three searches in succession are skipped for the rest of the cycle and probed again on the next cycle. Typed plugin deferrals use the provider cooldown and retry delay rather than becoming permanent failures.
- Explain acquisition results parked for review because their identity is ambiguous.

## Cleanup and deletion

- Completed and failed downloads use the same durable cleanup path. With **Remove Failed** enabled, failed Usenet downloads are removed with their files rather than removing only the client entry.
- Ask clients that support payload deletion to remove their own files first, then clean up eligible leftovers. The fallback inventory is taken before the client request, preserving files that appear afterward. For clients that remove only the entry, Scryer removes eligible files before removing the entry.
- Record whether the client or Scryer removed the payload, and avoid reporting files as removed when they were retained. Cleanup preserves newer downloads that reuse a client ID and retains finished import work when its ownership cannot be established.
- Deleting a title clears its unfinished imports, manual-import selections, and pending password retries while retaining completed import history. Clearing these records does not itself delete files from disk.
- Recover password retries whose jobs have left the client through **Activity > Import**, where removal ends the retry and requests client queue and history cleanup. Deleting a download client refreshes queue and tracking state immediately.
- Translate deletion refusals, including paths outside configured roots. Report recycle-bin purge failures separately from title deletion, and check current permissions for each title in a bulk deletion.

## Plugins and indexer configuration

- Plugin failures carry stable reason codes and translated explanations across indexer, download-client, notification, subtitle, queue, grab, and connection-test paths. Unknown codes preserve the plugin's detail or show a translated message naming the code. Credential failures identify the rejected credential type, and URL credentials are redacted from failure details.
- List integrations distinguish sign-in failures, missing lists, account blocks, and quotas; rate-limited lists resume after the plugin's retry delay.
- Indexer plugins can add value-dependent settings fields, hide built-in fields, display read-only notes, and request image CAPTCHAs. Notes are not saved or required. Added secret fields are stored and redacted as secrets, including while a plugin is blocked.
- Refresh dynamic fields and sign-in challenges when relevant inputs change, and clear fields belonging to a previous provider when switching provider types. CAPTCHA refresh and refused-save recovery reuse the associated sign-in session without storing the image.
- Declared endpoint hosts admit their subdomains, declared IP literal hosts are accepted, and the previous 64-endpoint declaration limit is removed. Plugin HTTP requests through a transport proxy return redirects to the plugin instead of following them automatically.
- Enforce indexer and archive memory limits across all memories in an invocation, and bound torrent filename extraction to contain oversized or malformed inputs.
- Update bundled Newznab and Torznab plugins to 3.1.0.

## Appearance and editing

- Choose a per-user palette: Scryer Default, Slate, Ocean, Forest, Plum, Ember, Graphite, or Papyrus. Refine light-palette contrast, navigation surfaces, editor styling, notifications, and artwork fades to match the selected palette.
- The sidebar's **Theme** menu offers palette and highlight-color controls, including custom colors and reset. Profile settings add **Glow**; new installs start with decorative glow off, while existing instances retain it.
- Dock the plugins panel beside centered page content when space permits. On narrower pages, open it from **Plugins** over the content with a backdrop. Keep it open across Lists tabs and refresh the catalog in place. Indexers now has **Indexers**, **Indexer Search**, and **Seeding Profiles** tabs.
- Add interpreter-aware script highlighting, environment-variable completions, snippets, bracket matching, and indentation assistance for shell, Python, PowerShell, batch, and Go scripts. Show rule validation feedback inside the editors.
- Preserve catalog state while navigating to titles, render restored tables whose saved position exceeds their current size, and keep row actions clickable above highlights. New titles refresh into the active catalog using its filters, and a missing library root no longer hides existing titles.
- Improve title and episode history on narrow screens, keep long folder names and file details inside their dialogs, preserve parent navigation through mobile submenus, and keep full-screen search controls clear of phone notches.
- Refresh the available application version before starting an upgrade. Raise default general API and mutation budgets to 3,000 requests per minute while retaining the 30-per-minute search budget and existing authentication limits.

## API and plugin compatibility

- Remove `createOperatorKeepClaim`, maintenance candidates' `operatorKeepClaimId`, `LifecycleClaimKindValue.KEEP`, and the `REQUEST_PERMANENT` and `OPERATOR_KEEP` producer values. `convertTitleClaimToPermanent` now ends a finite lease.
- Add `reassignEpisodeFile` and its typed refusal reasons, and expose reassignment in activity and title history. Library root updates accept the stored root ID to preserve identity when changing paths.
- Add `CalendarEpisodePayload.airDateEstimated`. For movie events, `airDate` is the effective availability date.
- Add palette selection and `noGlow` to user UI settings, along with the palette enum values.
- Add `pluginConfigFields`, its sign-in challenge result and indexer/proxy context, provider `dynamicConfigTriggerKeys`, the `INFO` field type, and the `LOGIN_CHALLENGE_ANSWER` field role. Add `HTTP_RESOURCE` to download source kinds and recycle-bin outcome counts to title deletion results.
- Report plugin failures as `PLUGIN_FAILED` GraphQL errors with `extensions.reason` and optional `extensions.detail`; REST responses use status `502`.
- Plugin SDK 3.17.0 adds HTTP source/input kinds, capabilities, and release payloads. SDK 3.18.0 adds `config_fields`, the optional dynamic-config capability, and open string error codes. SDK 3.19.0 adds informational fields, CAPTCHA answer roles, and `checkCaptcha`. Existing plugin payloads remain supported; Rust SDK users constructing changed structs as literals must include their new optional fields when updating.
