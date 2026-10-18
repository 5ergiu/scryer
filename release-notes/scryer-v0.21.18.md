# Scryer 0.21.18 release notes

These changes are relative to v0.21.17.

## Highlights

- Control movie acquisition by release availability, with regional release dates, per-title thresholds, and an adjustable date offset.
- Validate operator-selected additional files and manual series-movie imports before transfer, while preserving manual overrides. Improve pack completion and imports with long or truncated filenames.
- Recover title records that incorrectly claim a library root or parent folder, copying verified media into title folders while retaining the original files.
- Scope delay profiles to individual libraries and make scores and scoring explanations consistent across search, files, history, and Wanted.

## Movie availability and acquisition

- Add **Announced**, **In Cinemas**, and **Released** movie availability thresholds. Movies without an explicit title override use the instance default, which defaults to **Released**.
- Choose a release market and an availability offset in days. Negative offsets permit earlier acquisition; positive offsets delay it.
- Use theatrical, digital, and physical dates for the selected market. When no home-release date is supplied, a known theatrical date can provide an explicitly marked 90-day estimate. Unknown or unusable dates keep automatic acquisition waiting.
- Show whether a movie is available, waiting, or awaiting release-date information, together with its effective date and reason. Interactive queuing before availability requires explicit confirmation.
- Keep movie availability restrictions off movies linked to a series. Reject episodic releases and releases with a conflicting year when automatically selecting a standalone movie, and hold automatic movie imports that contain a set of episodes.
- Keep automatic single-episode searches from selecting season or multi-episode packs outside the requested coverage.

## Imports and download completion

- Run the existing media analysis and content checks before transferring additional episode, movie, and series-movie files, and before dedicated manual series-movie imports. Reuse the accepted analysis for naming and stored media details.
- Preserve operator-selected additional files even when their score is lower, scoring rules reject them, or required audio languages are missing. Existing manual runtime and probe-error overrides remain available, and source-change protection still applies.
- Revalidate a timestamp-only source change once when file identity and content remain unchanged, reducing unnecessary held imports after download-client metadata updates. Preserve the original diagnostic when the probe detects a real source change.
- Complete episode packs when another member is already covered by a verified better library copy. Retain incoming and incumbent files, avoid blocklisting the release, and keep missing incumbent files from incorrectly satisfying the pack.
- Recover truncated filenames using names recorded at grab time and, when needed, the original posted filenames supplied by Weaver. Preserve episode information at the end of unusually long release names and keep staging filenames within filesystem limits.

## Folder recovery and file preservation

- Automatically enroll recovery operations for title records whose folder ownership incorrectly includes a library root or its ancestor. Copy verified media into an appropriate title folder and retain the original source files.
- Record recovery progress durably so interrupted operations can resume. Recheck ownership, source identity, and destination conflicts before accepting recovered files; unresolved cases remain available for review.
- Include recovery staging records in backups and restores so recovery progress is preserved with the catalog.
- Identify pending imports blocked by invalid title-folder ownership more clearly.
- Allow catalog-only removal when a title's folder cannot be established. For explicit file deletion, derive a missing folder only when recorded media identifies one valid, claimable title folder; shared-folder and ownership safeguards still apply.

## Delay profiles, languages, and scoring

- Limit delay profiles to selected libraries alongside their existing facet, tag, and priority filters. Profiles without a library selection continue to apply across libraries, and existing profiles are migrated automatically.
- Apply audio-language inference consistently during acquisition, import scoring, and stored-file score refreshes, including anime language context.
- Expose `release.indexer_languages` to custom rules. This normalized list contains only languages reported by the indexer, separately from languages inferred from filenames or title metadata.
- Use a shared score display and scoring log across release search, library files, title history, Wanted, and rule tests. Retain rule origins so explanations can distinguish user rules from system rules.

## Lists and interface improvements

- Combine **Add and search** into **Add** for followed lists. Route monitoring now determines whether added titles enter the normal acquisition cycle. Existing search-mode subscriptions are migrated to Add.
- Open interactive release search in a dialog and improve mobile result cards, including tappable scores, scoring logs, and queue controls.
- Adapt episode layouts to the width of their season section, preserve episode selectors in responsive cards, and show file size in its own column.
- Make catalog select-all include every matching title and align score refresh with the other bulk actions.
- Improve history event filters and request filters, retain Wanted sidebar counts while refreshed counts load, and show plugin **Update All** only when updates are available.
- Present library root folders and download-client routing as cards on smaller screens, and improve sidebar badge alignment and facet ordering.

## Compatibility and upgrade notes

- GraphQL clients must use `ListMode.ADD` in place of the removed `ListMode.SEARCH` value. List-title-added event data no longer includes the `searched` field; acquisition follows the route's monitoring settings.
- Add GraphQL fields for movie availability settings and status, explicit early-queue confirmation, delay-profile library selections, and score-entry origins. Location-operation enums also include title-folder recovery and source-preserving copy operations.
- Fix PostgreSQL decoding of movie release dates stored as text.
- Ensure Linux musl builds include the cryptographic entropy fallback for kernels that report `getrandom` as unsupported.
