# Scryer 0.21.17 release notes

These changes are relative to v0.21.16.

## Highlights

- Discover titles in existing nested library layouts without treating organizing folders as individual titles. Scans preserve existing title identity and retain file records when discovery is incomplete.
- Inspect current file scores, score breakdowns, and acquisition decisions, with controls to refresh scores for selected files, seasons, or titles.
- Recover missing Windows Installer sources through a new setup executable and retain verified installers for future upgrades and repairs.

## Library discovery and folder safety

- Support titles at different depths within the same library, including organizing folders grouped by letter or year. Resolve title identity separately from folder boundaries so a folder-name mismatch alone does not reject otherwise usable media.
- Keep conventional season folders, disc folders, and bonus subfolders with their parent title instead of discovering them as separate titles. Preserve explicitly owned linked title folders during discovery.
- Use local identity and external ID hints when matching existing media, and preserve existing scan state when a scan is incomplete or canceled. Background refresh boundary discovery now honors scan cancellation.
- Strengthen folder ownership checks for renames, moves, and deletion. Refuse operations on shared organizing folders, overlapping title folders, library roots, or paths outside a title's owned folder.
- Require verified recorded media before deleting a title's folder. Refusal messages explain whether to correct the title folder or rescan it before trying again.
- Support organizing paths in title-folder templates, with language-aware article sorting and first-character naming tokens. Rename previews show the naming language and how many titles change organizing folders.
- Require the last folder in a title-folder template to include a title or external ID token. Reject empty and dot path components consistently in the browser and backend.

## Scores, search, and downloads

- Show current file scores separately from the score recorded at acquisition, including evaluation time, refresh status, time-sensitive results, rejection reasons, and scoring contributions.
- Add background score refresh jobs for selected files, seasons, and titles, with progress and cancellation. Retain completed evaluations if a refresh is interrupted.
- Display score changes and decision evidence in title history, and identify current files in release search results. Fall back to the on-disk filename when the original release name is unavailable.
- Recognize numbered PROPER and REPACK revisions, RERIP, and REAL revision markers. Apply automatic revision preference only to related releases from the same release group and quality tier. The new **Ignore PROPER/REPACK** quality-profile option lets quality and scores decide instead; custom scoring rules can still reward revisions.
- Make manual failed-download actions explicit about blocklisting and deleting download data. Preserve downloads when the blocklist entry cannot be confirmed, and report missing release identity instead of repeatedly retrying an unsafe cleanup.
- Apply the small-file plausibility check using probed runtime when catalog runtime is unavailable.

## Windows installation and upgrades

- Add a setup executable that restores missing installer information and verifies the required downloads before starting Windows Installer.
- Retain verified MSI sources outside disposable upgrade staging so later repairs, removals, and upgrades can locate the original installer.
- Improve shutdown of the installed tray and server during upgrades, including servers adopted by the tray. Handle Windows session shutdown and installer-triggered tray restart.
- Fix the installer process-stop command for custom installation paths containing an apostrophe.

## Other improvements

- Remember catalog filters separately for movies, series, and anime in browser storage.
- Label list ordering as **Source rank**, clarify paginated entry ranges, and avoid displaying the release year twice in list titles.
- Add support for indexer plugins to declare their configured HTTP(S) endpoints before network access, with endpoint validation and host-policy error reporting.
- Recover expired plugin compilation cleanup markers so stale cache state does not prevent plugin compilation.
- Update rustls to 0.23.46.
