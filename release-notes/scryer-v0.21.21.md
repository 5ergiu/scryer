# Scryer 0.21.21 release notes

These changes are relative to v0.21.20.

## Highlights

- Add the HTTP protocol family for indexers and a native HTTP download client, so indexer plugins can offer provider pages, direct files, and streaming manifests that Scryer downloads itself.
- Request leases are now only a time to delete. The one-click **Keep** and operator keeps are removed, and a live lease no longer protects a title from other maintenance rules. Protect titles with tag-driven rules instead.
- When a download is removed with its files, a client that can delete its own files is always asked to, and Scryer then removes whatever it left behind. Failed Usenet downloads are removed with their files under **Remove Failed**.
- Explain plugin failures in the viewer's language, and let indexer plugins add or hide settings fields based on the values entered so far.
- Rework the plugins panel on Settings, Indexers, and Lists into a right-edge panel, and move the Indexers page's sections into tabs.
- Add a per-user **Glow** preference. New installs start with the decorative glow off; existing instances keep it.

## HTTP acquisition

- Indexer plugins can declare the HTTP protocol and return HTTP releases. Release searches show these results with the **HTTP** protocol, and a grab is routed to a download client that accepts at least one of the release's transports. Releases the indexer marks live, upcoming, removed, DRM-protected, or sign-in gated are refused when grabbed, on every submission path. A grab whose HTTP details are no longer in the search listing fails with a clear error.
- Add a built-in **HTTP** download client type that fetches direct-file resources itself. Configure its download folder, concurrent downloads (1 to 8), connections per host (1 to 8), and a shared bandwidth limit. The client picks the best complete media file within the quality profile's ceiling plus requested subtitle sidecars, streams into a `.part` file that is renamed on completion, resumes interrupted transfers, honours `Retry-After`, keeps resource headers on their own origin across redirects, and routes through the plugin egress policy or the assigned proxy. Its queue is persisted under `.scryer-http` inside the download folder so transfers resume after a restart; nothing is deleted, including partial files.
- A `403` answer is treated as a possible geo-block: the same variant is requested up to three times, then up to three lower-ranked variants are tried before the job fails with a summary of the refused variants and request count. Other `4xx` answers fail without retry.
- Finished files are probed before they take their final name. A job fails when no video track is present or the duration falls short of what the indexer declared. Request errors are recorded without their URL so signed CDN tokens never reach the queue, logs, or UI.
- HTTP grabs keep their Scryer identity through import, so a release that carries only an air date is filed under the linked catalog episode's season.
- Indexers whose plugin fixes its own endpoint, and so has no connection URL field, can be created and enabled. HTTP indexer plugins are listed under **Indexers** in Settings.

## Request leases and maintenance

- A request lease is now only the time at which a requested title becomes due for deletion. A live lease no longer stops other maintenance rules from acting on the title; protecting a title is the job of a rule, for example one that skips titles with a tag.
- A request approved **forever** or with a zero-day lease no longer creates a lease, so its title never appears to lease-based rules such as **Expired request leases**.
- Remove the one-click **Keep** from maintenance candidates, the title page, and the Requests claims panel, along with the **Kept** badge and its release control. Operator keeps no longer exist.
- **Keep permanently** on a request now ends its lease, leaving the title with no expiry. It is refused for a request that has no finite lease.
- The built-in **Expired request leases** rule now matches every title whose lease has expired. Its grace period, libraries, and storage root are kept; if it was **Enabled**, it returns to **Dry run** on upgrade so its new candidates can be reviewed before it is enabled again.
- Rules can no longer read the `keep_claim_active` fact. Edit any custom rule that reads it.

## Download cleanup

- Completed and failed downloads share one cleanup path. A failed Usenet download is no longer removed from the client at failure time while its files are left behind; when **Remove Failed** is on, it is removed together with its files.
- When cleanup removes a download with its files, SABnzbd, Weaver, and download-client plugins that can delete their own data are always asked to delete the files. Scryer then removes what the client left in the job folder, deleting only files that were already there before the client was asked, so content that appears in the folder afterwards is kept. NZBGet and torrent plugins that remove only the entry still have their files deleted by Scryer before the entry is removed.
- Cleanup records whether the client or Scryer removed the files, and no longer reports a payload as removed when files were kept.
- Removing a download no longer removes a newer download that the client has since given the same id, and finished import work is kept while its ownership cannot be read.
- Deleting a download client refreshes the queue and download tracking at once.

## Title deletion

- Title deletion is refused, before anything is deleted, while a download password retry is still awaiting reconciliation. A download whose job has left the client during such a retry appears in **Activity > Import**, where removing it ends the retry and asks the client to remove the job from its queue and history.
- Explain every title, media file, episode file, and subtitle deletion refusal in the viewer's language, including a distinct message for a path outside every configured root.
- Report recycle-bin entries that could not be purged after a title deletion separately, instead of treating the title deletion as failed.
- Bulk title deletion checks the requesting user's current permissions when each title is deleted.

## Plugins

- Plugin failures carry a stable reason. Indexer and download-client connection tests show a translated explanation, followed by the plugin's own detail when it gave one. A reason this version of Scryer does not recognise shows the plugin's detail, or a translated sentence naming the code.
- An indexer search refused for an invalid API key, passkey, cookie, or username and password names the rejected credential.
- Lists treat refused credentials and account blocks (captcha, two-factor, banned, disabled) as sign-in problems, a missing list as not found, and an exceeded quota as a rate limit that resumes after the plugin's retry delay.
- Indexer plugins can add settings fields, or hide built-in ones, based on the values entered so far. The indexer editor asks the plugin again when one of those values changes, and secrets in added fields are stored and redacted like any other.
- Enforce the indexer and archive plugin memory limits across all of a plugin's memories combined, not each memory separately.
- Bound the work spent reading file names from a grabbed torrent, so an oversized or malformed torrent can no longer exhaust memory.

## Interface

- The plugins panel on Settings, Indexers, and Lists is a 320 px panel on the right edge. It docks beside the centred page content when there is room and stays open across Lists tabs; on narrower pages, the **Plugins** button opens it over the content with a backdrop. The list refreshes in place when the catalog changes, and a fade and arrow show when more plugins are below.
- The Indexers page shows **Indexers**, **Indexer Search**, and **Seeding Profiles** as tabs under the page header.
- Add a **Glow** switch to profile settings to turn off the decorative accent glow.
- Newly added titles appear in the catalog without a reload when they match the active filters. A missing library root no longer hides a catalog that has titles.
- The application upgrade control re-reads the available version before starting, so a page opened during the startup update check no longer requests a stale version.

## Imports, parsing, and acquisition

- Automatic import recognises a video file with no extension by its content, as manual import already did, and gives the imported file the matching extension.
- When an anime release's community season number is also a real season, Scryer keeps the literal reading unless the alternate one is corroborated by forced alternate numbering, the episode the release names, or a date near the episode's air date.
- Unconfirmed grabs are released only when the download client's history reaches back far enough to cover the grab. A client with long history no longer causes a grab it accepted to be released and downloaded again.
- Title acquisition diagnostics explain releases parked for review because their identity is ambiguous.

## Compatibility and upgrade notes

- Migrations delete every operator keep, and every lease from a request approved forever or with a zero-day lease. Leases with an expiry, including ones an administrator extended to a specific date, are kept. Titles that relied on an operator keep or a live lease for protection from maintenance rules are no longer protected; add a tag-driven rule before enabling destructive rules.
- If **Expired request leases** was **Enabled**, re-enable it after reviewing its candidates in **Dry run**.
- Weaver releases before 0.11.5 are not supported.
- GraphQL: `createOperatorKeepClaim` and the candidates' `operatorKeepClaimId` field are removed. `LifecycleClaimKindValue.KEEP`, `LifecycleClaimProducerValue.REQUEST_PERMANENT`, and `LifecycleClaimProducerValue.OPERATOR_KEEP` are removed. `convertTitleClaimToPermanent` now ends the lease instead of creating a keep.
- GraphQL: add the `pluginConfigFields` query and `dynamicConfigTriggerKeys` on provider types. Title deletion results add `recyclePurged`, `recyclePreserved`, `recycleBusy`, and `recycleFailed`. User UI settings add `noGlow`. `DownloadSourceKindValue` gains `HTTP_RESOURCE`.
- Plugin failures are reported as `PLUGIN_FAILED` GraphQL errors with `extensions.reason` and, when the plugin gave one, `extensions.detail`. The REST API answers these with `502`.
- Plugin SDK 3.17.0 adds the `Http` indexer source kind, the `HttpResource` download input kind, HTTP capability blocks for indexers and download clients, and an optional `http` payload on `PluginDownloadSource`.
- Plugin SDK 3.18.0 adds the `config_fields` indexer action and the optional `dynamic_config_fields` capability, and opens `PluginErrorCode` to any string while keeping its well-known codes. Plugins built against earlier SDKs keep working; Rust SDK users constructing these structs as literals must include the new optional fields when updating their SDK.
- Migrations add HTTP release storage to pending releases, the per-user glow preference, and the lease changes described above.
