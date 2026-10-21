# Scryer 0.21.20 release notes

These changes are relative to v0.21.18. Release 0.21.19 was withdrawn shortly after it was published; everything it contained ships here instead, with maintenance rules and request rules kept experimental.

## Highlights

- Maintenance rules and request rules stay experimental. Both panes require the experimental features switch, no rule is enabled by default, and instances that ran 0.21.19 without that switch have every rule disabled on upgrade.
- Give each maintenance rule one **Disabled**, **Dry run**, or **Enabled** state, controlled together with two instance switches: **Maintenance** and **Pause all actions**. Request rules are enabled with a single checkbox.
- Ship a built-in maintenance rule, **Expired request leases**, that deletes requested titles seven days after their request lease expires unless they are kept. It is created **Disabled**.
- Keep a title with one click from maintenance candidates, the title page, or the Requests claims panel.
- Add the HTTP protocol family for indexers and a native HTTP download client, so indexer plugins can offer provider pages, direct files, and streaming manifests that Scryer downloads itself.
- Run scheduled jobs independently, so a long RSS sync no longer delays housekeeping, health checks, Prowlarr sync, or pending-release processing.
- Bound RSS polling and indexer plugin memory, and process feeds as they arrive instead of retaining every indexer's results for the whole cycle.
- Configure movie availability on the Movie facet and override it per library. Manual queueing bypasses availability thresholds; automatic acquisition continues to enforce them.

## Maintenance and request rules are experimental

- The maintenance rules and request rules sections, their sidebar entries, and their command-palette routes require the experimental features switch, as in 0.21.18. Rules, exclusions, candidates, and recorded decisions created on 0.21.19 are kept.
- No rule is enabled by default. The built-in **Expired request leases** rule is created **Disabled** on new instances; enable it on purpose, with the usual candidate-count confirmation, once experimental features are on.
- On upgrade, every maintenance rule becomes **Disabled** and every request rule is disabled unless the instance already had experimental features enabled. Instances that had opted in keep their rule states, subject to the mode changes in the upgrade notes below. Re-enable rules from the experimental rules sections after upgrading.

## Movie availability

- Move the default availability threshold, release market, and date offset from Acquisition settings to the Movie facet's **General** section. Movie libraries can override each setting independently; a title's own threshold still takes precedence.
- When the selected market has no release date, fall back to worldwide release dates. For **Released**, use a worldwide digital or physical date before estimating from a theatrical date plus 90 days. For **In Cinemas**, use a worldwide theatrical date before falling back to a home-release date. Dates taken from another market are marked as estimated and explain their source; unusable home-release dates still hold automatic acquisition.
- Queue manually selected releases without an availability confirmation or refusal. Release results and the grab dialog show when automatic search is waiting for availability; RSS and other automatic acquisition paths still enforce the threshold.
- Refresh affected movies' release dates when their effective release market changes. Show the selected library's effective threshold when adding a movie, and keep the availability controls from overlapping at narrower widths.

## HTTP acquisition

- Indexer plugins can declare the HTTP protocol and return HTTP releases. Release searches show these results with the **HTTP** protocol, and a grab is routed to a download client that accepts at least one of the release's transports. Releases the indexer marks live, upcoming, removed, DRM-protected, or sign-in gated are refused when grabbed, on every submission path.
- Add a built-in **HTTP** download client type that fetches direct-file resources itself. Configure its download folder, concurrent downloads (1 to 8), connections per host (1 to 8), and a shared bandwidth limit. The client picks the best complete media file within the quality profile's ceiling plus requested subtitle sidecars, streams into a `.part` file that is renamed on completion, resumes interrupted transfers, honours `Retry-After`, keeps resource headers on their own origin across redirects, and routes through the plugin egress policy or the assigned proxy. Its queue is persisted under `.scryer-http` inside the download folder so transfers resume after a restart; nothing is deleted, including partial files.
- A `403` answer is treated as a possible geo-block: the same variant is retried up to three times, then up to three lower-ranked variants are tried before the job fails with a summary of the refused variants and request count. Other `4xx` answers fail without retry.
- Finished files are probed before they take their final name. A job fails when no video track is present or the duration falls short of what the indexer declared. Request errors are recorded without their URL so signed CDN tokens never reach the queue, logs, or UI.
- HTTP grabs keep their Scryer identity through import, so a release that carries only an air date is filed under the linked catalog episode's season.

## RSS and indexer reliability

- Poll at most two RSS indexers concurrently by default, separately from interactive searches. Process each completed feed before accepting more queued results, and release RSS plugin instances after each invocation.
- Limit each RSS poll to five pages, retaining at most the newest 500 results by default. Log and record when a poll reaches its ceiling so an uncovered catch-up window is visible.
- Give indexer plugins a separate 128 MiB memory limit and release retained instances above a 32 MiB watermark. Report memory-limit failures with the indexer name, and log invocation memory and retention decisions.
- Match RSS titles in batches using indexed external-ID lookups. Track seen release GUIDs separately for each indexer so polling one does not evict another's history.
- Queue indexer error history without waiting for database writes, preventing error recording from blocking acquisition and other writers. If the bounded history queue fills, log and count dropped entries while still counting every indexer error.

## Maintenance rules

- Each rule has a single state: **Disabled**, **Dry run**, or **Enabled**. **Dry run** evaluates the rule and records what it would do without acting; **Enabled** applies the rule's actions. Editing an enabled rule returns it to **Dry run** and marks it as edited since it was last enabled.
- Two instance switches appear above every maintenance pane. **Maintenance** controls whether rules are evaluated and their results shown on the instance. **Pause all actions** stops every rule from acting, reversible and destructive alike, while evaluation continues. The separate gates pane is gone; existing links to it open the rule list.
- Enabling a rule that deletes files shows the full list of current candidates in the confirmation.
- **Keep** a title from its candidate row, from the title page, or from the Requests claims panel. Maintenance rules do not delete a kept title or its files while the keep is in place, and the title shows as **Kept** with a control to release the keep. Keeping a title that is already kept returns the existing keep. Keeping a title requires title management permission on its library.
- Keep at most one live operator Keep per title, including concurrent clicks and conversions of request leases to permanent Keeps. Upgrade consolidates duplicate operator Keeps while preserving the title's protection.
- Exclusions are now listed under the candidates they were made from.
- The rule editor now lists name, template, matcher, action sequence, grace period, and libraries in that order. Rule scope and storage root sit under **Advanced**, which opens automatically when either has a non-default value.
- Rules can use list facts: which enabled lists hold a title, public and personal list counts, first and last seen dates, days on a list, best rank, on-leave and departure-handled status, and membership states. They can also read how a title was requested and the name of the public list it came from. Personal list names are never exposed. A fact that cannot be read is reported as unknown rather than treated as absent.
- Add two templates: **Left every list**, which unmonitors titles every list has dropped once the departures are handled and more than 30 days old, and **Added by a list and never watched**.

## Request rules

- A request rule is enabled with one checkbox. Requests are decided by the enabled rules that cover the request's library; there are no evaluation modes and no instance-wide gate.
- Rules that read requester fields still require the permissions-management right, and the rules page marks those rules.
- Rules can read the request's origin and, for requests that came from a list, the list's subscription, scope, mode, provider, owner policy, rank, and public name. If the list cannot be read, that section is omitted and the request is still evaluated.
- Add two templates: hold requests from lists that use the Approval policy, and deny personal-list requests above 1080p.

## Expired request leases

- Scryer now ships a built-in maintenance rule, **Expired request leases**. Seven days after a requested title's request lease expires, the rule deletes the title and its files unless the title has a Keep (a forever request or an operator Keep). It is created **Disabled** and, like every maintenance rule, only acts once an operator enables it while **Maintenance** is switched on and actions are not paused. When it is enabled, titles whose leases had already expired become candidates on the first evaluation pass and are deleted seven days later; Keep those titles first, or use **Dry run** to review the candidates. The rule cannot be renamed or deleted and its matcher and action cannot be changed; its grace period, libraries and state can be edited, and changing the grace period or libraries sets it to **Dry run** until it is enabled again.
- The rule is marked as built in on the maintenance rules list, and the request rules page includes a control for its state and grace period.

## Parsing and imports

- Recognize episode revision suffixes such as `S07E12v2` and `7x12v2` while keeping the season and episode, and record the revision so the release is treated as a proper upload.
- Leave revision-like text inside a known title as part of the title, and keep resolutions with a revision suffix from being read as episode numbers.
- During manual import, use the catalog's own numbering when a community anime season index collides with an official season and every file falls inside the grabbed episodes.
- Focus the search field when the manual import episode picker opens, and keep long episode selections from widening the import table.

## Downloads and recovery

- Recover unconfirmed grabs that otherwise leave titles stuck with active downloads and block further grabs or moves. After a 15-minute grace period, or after restarting Scryer, check a fresh download-client snapshot: bind a job that is present, retain the intent when the client cannot answer reliably, and release it only when its absence is confirmed. Record the failed grab and reopen its wanted scope without blocklisting the release. Move previews explain when a grab remains unconfirmed.
- Prevent download cleanup from blocking database writes by completing pending cleanup claims before applying their outcomes.
- Read queue age from download-client plugins that supply an `added_at` timestamp, allowing existing stalled-download recovery to apply to those items.

## Scheduling and jobs

- Run every scheduled job in its own task. A job that is still running when its next run is due skips that run rather than starting a second copy, and jobs running past their expected time are reported in the logs and metrics without being stopped.
- Clear a finished job run from the Jobs view even when its final status cannot be written, so the job no longer appears stuck and can be started again. The failed write is still logged and reported.

## Compatibility and upgrade notes

- Unless experimental features were already enabled on the instance, every maintenance rule becomes **Disabled** and every request rule is disabled on upgrade, including rules enabled on 0.21.19.
- On instances with experimental features enabled, maintenance rules that were in shadow mode become **Dry run** and need to be enabled again. Review maintenance behavior through the new **Maintenance** and **Pause all actions** switches.
- On those instances, request rules that were in shadow mode become **Disabled**. Enforcing rules also become **Disabled** when the retired instance gate was off or unset; those behind an enabled gate remain enabled. Review each rule's **Enabled** checkbox after upgrading.
- GraphQL rule controls have breaking changes. Replace `setMaintenanceRuleMode` and `setMaintenanceRuleArming` with `setMaintenanceRuleState`, and use `MaintenanceRuleState` (`DISABLED`, `DRY_RUN`, `ENABLED`) instead of the removed `MaintenanceEvaluationMode` type. Maintenance rule sets now expose `state`, `enabledArming`, and `editedSinceEnabled` instead of `enabled`, `evaluationMode`, `effectArming`, and `destructiveRearmRequired`. `MaintenanceInstanceGates` and its update input now use `enabled` and `actionsPaused` in place of the previous gate fields; `maintenanceCandidates` no longer accepts `includeShadow`.
- Replace `setRequestRuleMode` with `setRequestRuleEnabled`. The request-rule instance-gate query, mutation, and types are removed, along with `RequestRuleSet.evaluationMode` and `RequestRuleEvaluationModeValue.SHADOW`. Request decision and preflight modes still report `DISABLED` or `ENFORCE`.
- Existing acquisition availability settings migrate to the Movie facet, with existing facet values taking precedence. GraphQL clients should read and write them through `mediaSettings` and `updateMediaSettings` with Movie scope, or through library settings for overrides. The acquisition fields are deprecated: reads return facet values and writes are ignored. `QueueDownloadInput.overrideMovieAvailability` is also deprecated and ignored.
- Plugin SDK 3.17.0 adds the `Http` indexer source kind, the `HttpResource` download input kind, HTTP capability blocks for indexers and download clients, and an optional `http` payload on `PluginDownloadSource`. Older plugin payloads remain supported; Rust SDK users constructing these structs as literals must include the new optional fields when updating their SDK.
- `DownloadSourceKindValue` gains `HTTP_RESOURCE` in the GraphQL API.
- Download-client plugins may optionally return `added_at` as an RFC 3339 timestamp or Unix seconds. Older plugin payloads remain supported; Rust SDK users constructing `PluginDownloadItem` literals must include the new optional field when updating their SDK.
- RSS tuning is available through `SCRYER_RSS_INDEXER_CONCURRENCY` and `SCRYER_RSS_POLL_PAGE_BUDGET`. Indexer plugin limits can be adjusted with `SCRYER_INDEXER_PLUGIN_MEMORY_MAX_MIB` and `SCRYER_INDEXER_PLUGIN_RETAIN_WATERMARK_MIB`.
- Add Prometheus metrics for skipped overlapping scheduled runs, long-running scheduled jobs, and failed job-run status writes.
- Migrations add HTTP release storage to pending releases and apply the rule reset described above.
