# Scryer 0.21.16 release notes

This patch release fixes search stalls and lost results during large paginated searches, preserves managed indexer routing settings, and improves imports, Lists, requests, script settings, and Windows upgrades. These changes are relative to v0.21.15.

## Highlights

- Large searches replay stored candidates in bounded batches and retain complete indexer pages, including results beyond the requested page size.
- Indexer error recording no longer blocks the runtime workers needed to finish database and network operations.
- Prowlarr and Jackett syncs preserve existing routing categories, enabled state, and priority.

## Bug fixes

### Search and indexers

- Replay stored search candidates in batches of 100, waiting for scoring capacity before loading the next batch instead of loading all candidates up front.
- Deduplicate candidates shared across search runs while advancing through stored results with a cursor.
- Preserve the tail of oversized plugin result pages, including the final page, so those releases remain available for scoring.
- Prevent runtime starvation when several indexer failures need to be recorded at once.
- Keep operator-configured routing on existing managed indexers during sync, including deliberately empty category selections. Default routing is seeded only for new entries.

### Imports

- Stop rejecting an otherwise successful import solely because the destination reports a different owner, as can happen with mapped storage ownership. Ownership differences are logged for diagnosis; existing copy verification and file-preservation safeguards remain in place.
- Display probed runtimes as clock durations, such as `24:55` or `1:02:05`, in import rejection and replacement-block messages.

### Lists

- Remove the remaining experimental gates from Lists surfaces and permissions.
- Combine Trakt season information across pages before deduplicating shows. A show appearing with different seasons is retained without incorrectly restricting it to the first season encountered.
- Spread automatic Simkl subscription syncs across a deterministic five-to-seven-hour interval instead of scheduling every subscription at the same interval.
- Add Simkl attribution and links across catalog previews and other list surfaces.

### Requests

- Reopening a collapsed request row now reopens every request represented by that row.
- Clear requester and library filters when switching accounts so filters from the previous account do not hide the new account's queue.

### Scripts

- Resolve each interpreter setting independently. A failed settings read for one language no longer discards the configured paths for other languages.
- Preserve interpreter paths loaded after the settings form opens for fields the operator has not edited, while retaining edits already made.

### Windows

- Stop the installed tray application before the upgrade checks for files in use, preventing the tray from blocking replacement of installed files, including during silent upgrades.
