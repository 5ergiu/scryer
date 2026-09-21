# Scryer 0.21.8 release notes

These notes cover what's changed since **0.21.7**.

## Highlights

- **Scryer uses far less memory.** Scryer now manages its own memory rather than leaving it to the system, on every platform. On a test library of about 111,000 items, memory use used to climb to roughly 3 GB and never level off; it now settles at about 500 MB and stays there. Releasing unused memory was also moved onto its own background threads, which brought average memory during the same test down from 687 MB to 532 MB and Scryer's CPU usage. Nothing needs configuring.

- **Matching a file to a title no longer keeps a second copy of your library in memory.** Matching now reads a search index kept in the database instead of a copy of the catalog held in memory. Three things come out of that:
  - Memory use is lower again on large libraries, and RSS checks no longer rebuild a library-sized index every cycle.
  - Renaming a title, changing what it's monitored for, or adding a title now affects matching straight away. Before, there was a window of up to a minute in which matching still used the old information.
  - The same index backs search, so what you can find and what Scryer can match now agree.

- **Titles match and search better across languages and spellings.** The index stores several spellings of each title, so a search or a filename finds the title whichever way it's written: German umlauts and ß (`Müller`, `Mueller`, `Muller`; `Straße`, `Strasse`), Japanese titles written in kana, kanji or romanized form with their common variants, Chinese and Korean titles, Cyrillic titles and their Latin transliterations, and titles that mix the two. Roman numerals are now read from the way the title is actually spelled, so a title that genuinely contains a word like `Mix` is no longer rewritten as a number.

- **Interactive indexer search and grabbing have been reworked.** When you grab a release you now pick the download client and the category to send it to, with **Client default** available for both, and Scryer shows which client the release would go to before you commit. **Grab & Assign** attaches the release to a library title in the same step, with suggested titles offered for you to pick from. The results table gained a **Peers / Grabs** column and the release year, and the categories Scryer searches are a better fit for what you asked for.

- **Indexers and download clients can be edited in place in Settings.** Enabling or disabling one, changing its routing, and assigning a proxy are all done directly in the table now, with a routing table shown per indexer and save progress shown on the row you're editing.

- **New settings panel: Trusted proxies for rate limiting.** Enter the IPv4, IPv6 or CIDR addresses whose `X-Forwarded-For` header Scryer should trust when identifying clients for rate limiting. Changes apply immediately without a restart, a saved list overrides `SCRYER_RATE_LIMIT_TRUSTED_PROXY_IPS`, saving an empty list trusts no proxies, and **Use environment/default** returns to the environment setting. This affects rate limiting only — it does not change login rules, local access, or rate-limit bypasses.

- **Imports can be retried from history.** Each import history row now offers **Retry import**, which re-evaluates the files that were downloaded using the title and quality profile you have now, and **Retry import with password** for a download that needs one.

- **Quality profiles: drag to set quality preference.** Tiers are ordered highest preference first, tier preference takes priority over score, and a newly added tier starts at the lowest preference.

- **The dashboard is faster on large libraries**, keeps a title's artwork visible while you hover it, and marks a title that has just been imported with a **New import** badge.

## Included fixes

- **Scans:** a year in brackets before the episode number, the way some anime is named (`Example Show (1992) - S06E28`), was read as an absolute episode number, so the file could not be placed and was left out of the library. On a fresh scan of a 38,077-file test library this left 474 files unplaced; after the fix, none.
- **Imports:** when a finished download could not be imported yet and was scheduled to be tried again, that schedule was only held in memory, so restarting Scryer either forgot it or retried immediately. It is now stored, and the order of quality tiers is preserved when the files are re-evaluated.
- **Quality:** the quality shown for a file now uses the same size thresholds the scan itself uses, so video that has been cropped or padded — common widescreen and vertically padded HD frames — keeps its proper tier instead of being labelled one step lower. 1440p and 4320p now have labels of their own.
- **Catalog:** paging through the catalog no longer works out library-wide totals that were not asked for, and the list no longer loads artwork details it does not display. Both made large libraries slower to browse.

## API changes

These affect scripts and tools that call Scryer's GraphQL API. The Scryer web app is already updated.

- **Added:** `indexerGrabClients(searchId, downloadUrl, titleId)` returns the download clients a release from an interactive search may be sent to, as `IndexerGrabClientPayload` (`id`, `name`, `category`, `mapped`).
- **Added:** `downloadClientCategories(clientId)` returns a client's configured categories as `DownloadClientCategoriesPayload` (`supported`, `categories`). Clients that cannot report categories return `supported: false`, and a category can still be typed in by hand.
- **Added:** `queueIndexerSearchAssignment(input, routing, replacement)` grabs a release and assigns it to a catalog title in one call. `routing` takes the new `IndexerGrabSelectionInput` (`clientId`, optional `category`; omitting the category uses routing, an empty string uses the client's default).
- **Added:** `QueueUnlinkedReleaseInput` accepts an optional `category`, with the same meaning.
- **Added:** `ParsedReleasePayload` has a `year` field taken from the release parser.
- **Added:** `ServiceSettingsPayload` has `trustedProxyIps` (the addresses in effect), `trustedProxyOverride` (the saved list, or null when the environment is in use) and `trustedProxySource`.
- **Added:** `UpdateServiceSettingsInput` accepts `trustedProxyIps` to save a list and `resetTrustedProxyIps` to clear it and return to the environment setting.
- **Changed:** `UpdateServiceSettingsInput.tlsCertPath` and `tlsKeyPath` are now optional. Leaving either out keeps the saved path; previously both had to be sent on every update.

## Upgrading

Scryer updates its database automatically the first time the new version starts. No configuration changes are required.

Two changes are made on first start: the multilingual title spelling index is built, and a new index is added for the dashboard. On a large library the first start can take a little longer than usual while the title index is built. PostgreSQL installations build the same index when Scryer starts.

Expect Scryer to use noticeably less memory than before. If you have set a memory limit for your container based on the old behaviour, there is nothing you need to change, but you may find the limit is now far higher than it needs to be.
