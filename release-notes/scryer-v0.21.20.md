# Scryer 0.21.20 release notes

These changes are relative to v0.21.19. Release 0.21.19 was withdrawn shortly after it was published. If you are upgrading from 0.21.18, the 0.21.19 release notes also apply, except that maintenance rules and request rules remain experimental.

## Highlights

- Maintenance rules and request rules are experimental again. Both panes, their sidebar entries, and their command-palette routes require the experimental features switch, as before 0.21.19.
- Add the HTTP protocol family for indexers and a native HTTP download client, so indexer plugins can offer provider pages, direct files, and streaming manifests that Scryer downloads itself.

## Maintenance and request rules

- The maintenance rules and request rules sections are hidden unless experimental features are enabled. Rules, their states, exclusions, candidates, and recorded decisions created on 0.21.19 are kept and work unchanged once the switch is on.
- The rule engine is not changed by this release. Rules that were enabled on 0.21.19 continue to be evaluated and to act while the instance's **Maintenance** switch is on and actions are not paused, including the built-in **Expired request leases** rule. An instance that never turned **Maintenance** on evaluates nothing.

## HTTP acquisition

- Indexer plugins can declare the HTTP protocol and return HTTP releases. Release searches show these results with the **HTTP** protocol, and a grab is routed to a download client that accepts at least one of the release's transports. Releases the indexer marks live, upcoming, removed, DRM-protected, or sign-in gated are refused when grabbed, on every submission path.
- Add a built-in **HTTP** download client type that fetches direct-file resources itself. Configure its download folder, concurrent downloads (1 to 8), connections per host (1 to 8), and a shared bandwidth limit. The client picks the best complete media file within the quality profile's ceiling plus requested subtitle sidecars, streams into a `.part` file that is renamed on completion, resumes interrupted transfers, honours `Retry-After`, keeps resource headers on their own origin across redirects, and routes through the plugin egress policy or the assigned proxy. Its queue is persisted under `.scryer-http` inside the download folder so transfers resume after a restart; nothing is deleted, including partial files.
- A `403` answer is treated as a possible geo-block: the same variant is retried up to three times, then up to three lower-ranked variants are tried before the job fails with a summary of the refused variants and request count. Other `4xx` answers fail without retry.
- Finished files are probed before they take their final name. A job fails when no video track is present or the duration falls short of what the indexer declared. Request errors are recorded without their URL so signed CDN tokens never reach the queue, logs, or UI.
- HTTP grabs keep their Scryer identity through import, so a release that carries only an air date is filed under the linked catalog episode's season.

## Compatibility and upgrade notes

- Plugin SDK 3.17.0 adds the `Http` indexer source kind, the `HttpResource` download input kind, HTTP capability blocks for indexers and download clients, and an optional `http` payload on `PluginDownloadSource`. Older plugin payloads remain supported; Rust SDK users constructing these structs as literals must include the new optional fields when updating their SDK.
- `DownloadSourceKindValue` gains `HTTP_RESOURCE` in the GraphQL API.
- A migration adds HTTP release storage to pending releases.
