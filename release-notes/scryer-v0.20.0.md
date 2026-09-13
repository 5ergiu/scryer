# Scryer 0.20.0 release notes

## A quick note about experimental features

I've marked the experimental features below. To try them, enable **Experimental features** in **Settings → General**; it's off by default. Turning it off again hides the controls but doesn't stop running jobs or disable armed rules.

Experimental features are subject to change at any time, and **they will change materially over the coming weeks**.

There's a lot in 0.20.0. Alongside the new ways to organize and manage your library, this release brings more detailed media information, better release selection, new proxy options, and fixes throughout the app. These notes cover what's changed since **0.19.17**.

## What's new at a glance

- **Move and combine libraries — Experimental.** Move a few titles or a whole root folder, see what will happen first, and pick up interrupted moves from Activity.
- **Put routine library work on a schedule — Experimental.** Maintenance rules can use tags, storage information, and Jellyfin watch history to decide what to keep, unmonitor, or remove.
- **Set rules for requests — Experimental.** Automatically approve suitable requests, explain why others need approval, and choose how long requested titles should stay.
- **See more about your media — Standard.** Get richer video and audio details, clearer HDR and surround labels, and a way to inspect individual DVD and Blu-ray titles before importing.
- **Understand release scores — Standard.** Test rules against a real title and episode, see what contributed to a score, and get more consistent quality ranking.
- **Configure proxies in one place — Standard.** Reuse a connection across indexers and download clients, with SSH and WireGuard options and an outbound-IP test.
- **Get recommendations closer to your tastes — Standard.** Discovery takes the mix of live action, animation, and anime in your library into account.
- **An updated plugin system — Standard.** Scryer checks existing plugins for compatibility and tells you when one needs an update.

## Before you upgrade

These changes apply whether or not you enable experimental features.

### Some plugins will need an update

The plugin system has changed in 0.20.0, and plugins built for the old system won't necessarily work with it.

Scryer will try to find a compatible replacement from its bundled plugins or the plugin's original source. It keeps your configuration and whether you had the plugin enabled. A manually installed plugin won't be swapped for an unrelated plugin just because they share a name.

If a compatible replacement isn't available, Scryer keeps the existing plugin and settings and shows why it can't run. **Check your plugins after upgrading**, especially any you've installed manually. You may need an updated version from the original publisher.

### Discovery needs a compatible metadata gateway

The new recommendations rely on information from a newer SMG metadata gateway. If you run your own gateway, make sure it supports the new recommendation scores and library media mix before using this release. An older gateway will cause discovery sync to fail.

Some recommendation changes appear immediately; the new ranking information arrives with the next full discovery refresh. That refresh normally follows the daily schedule, though enough library growth can bring it forward. A manual sync doesn't force an early full refresh, so you may need to give the new ordering time to appear.

### Backups and the first few nights

Scryer updates its database at startup for the new features. These updates cover both SQLite and PostgreSQL, and include the information needed to remember moves, rules, and request retention.

Backups now include move progress and plugin compatibility information. Restoring a backup also rebuilds search and schedules missing title information to be filled in again, including anime numbering data. Scryer checks for plugin problems that would prevent a backup from being restored.

**Restore a backup using the same Scryer version that created it.** If you need to recover an older backup, restore it with that version first, then upgrade.

You'll also see background work to calculate full-file checksums for existing media. These help Scryer verify files and identify identical copies. The job runs overnight, remembers its progress, and doesn't need to finish before you can search or download. Artwork optimization is scheduled overnight too.

### New rules start disabled

Adding a rule doesn't immediately turn it loose on your library. You can first use Shadow mode to see what it would do. Request rules need enforcement enabled, and maintenance rules need the appropriate actions armed before they can make changes.

Rules that remove files need additional confirmation tied to the settings you reviewed. Changing those settings can require you to arm the rule again. A title may also be held because it's still protected by a request, its watch information is unavailable, or a move is already working on it.

## Moving and reorganizing your library

**Experimental:** moves, root changes, combining roots, and telling Scryer about files you've already moved. The import verification improvements described here are available without experimental mode.

### Move one title or a whole selection

Use **Move To…** to choose another root folder or a compatible library. You can move one title, select several, or work with titles from more than one library. If a title has no files yet, Scryer can simply change where it belongs.

Before you start, the preview shows where the titles are going, how their folders will be named, how much data is involved, and anything that needs your attention. If the destination already contains files or another copy of a title, you'll see how Scryer plans to handle it.

Titles that are busy downloading or importing may need to wait. Blocked titles stay visible in the preview with an explanation. If something changes after you've reviewed the move, Scryer asks you to review it again.

### Fix a title pointing at the wrong folder

**Change folder** lets you correct the folder associated with a title without moving its contents. This is useful when the files are already in the right place but Scryer has matched the wrong folder.

The picker shows whether another title already owns that folder. If it does, you can explicitly swap the two folders or take over the match. Scryer then updates the file associations while keeping the title's settings and identity.

### Already moved the files yourself?

Choose **Files are already there** in the move dialog. Scryer checks the destination and updates its records instead of copying everything again.

Every tracked file needs to be accounted for. Missing files or conflicting contents are shown for you to resolve. The old drive doesn't have to be available if Scryer can verify the destination.

Leftover source files aren't assumed to be duplicates just because their names match. Scryer needs proof before treating a copy as redundant.

### Move a root folder or combine two roots

You can change the path of a saved root folder and move its titles with it. You can also combine one root with another in the same library—for example, when bringing content from an older drive onto a larger one.

The preview covers the affected titles, files, and naming conflicts. A whole-root move won't silently leave a blocked title behind. If you're combining the default root with another root, the destination becomes the new default for future titles.

Recycle-bin information follows the root move. Files Scryer can't account for are listed and left alone.

### Move between libraries, including libraries with the same title

Movies can move between movie libraries. Series and anime can move between compatible series and anime libraries. The preview explains how the destination's settings and folder naming will affect the title.

If the destination already has the same title, Scryer can combine the two entries. It checks the title's metadata identity rather than trusting a matching name. If it can't confidently match the title or its episodes, it asks for help.

The destination keeps its settings and existing primary files. Tags, history, requests, and compatible files are brought across. Incoming episodes can fill gaps, while another copy of an episode you already own can be kept as an additional file.

Folder names follow the destination library's naming rules. The move doesn't automatically rename every media file; you can use the normal rename preview afterwards if you'd like to do that too.

### What happens when files have the same name?

Scryer checks copied files before cleaning up their sources. When two files look like they might be identical, the move compares their full contents before treating them as duplicates. Matching names and sizes alone aren't enough.

If the contents differ, the incoming file gets a different name so it doesn't overwrite the destination. Subtitles, NFO files, artwork, and other related files follow the media they belong to.

The preview also warns about hardlinks when moving between filesystems, since the move can change how much disk space those files use.

For copies imported from a download client, you can choose **Full** or **Quick check** verification. Full is the default. That preference is separate from the checks used for library moves, and the result records which verification was applied.

### Follow a move and recover if it stops

You can watch a move in Activity, including progress for individual titles and files, the amount copied, and verification status. A live notification lets you keep an eye on it while using another part of the app. Completed moves also appear in history.

Progress is saved, so you don't need to keep the browser open. Interrupted work can be resumed or retried. Failed moves offer **Retry**, **Abandon**, and **Plan again**, depending on how you'd like to proceed.

If storage disconnects, Scryer checks for it to return and keeps related files together through waiting and retry states. Cancellation waits for in-progress file work to reach a safe stopping point.

While a title is moving, conflicting work on that title waits. When the move finishes, Scryer asks connected media servers to refresh the affected folders using your configured path mappings.

## Maintenance rules — Experimental

Maintenance rules let you describe routine library jobs once and have Scryer check for matching titles on a schedule. You can use title information, tags, available storage, where a title came from, and Jellyfin watch history to decide what should happen.

For example, a rule can stop monitoring a selection while keeping its files, change a quality profile and search when that profile changes, or add and remove tags. There are also explicit options for removing titles, seasons, or episodes. Season rules can check whether the parent show is empty before taking a follow-up action.

The editor includes starter templates and a preview. You can see which titles match and why an action was skipped, held, or completed. Saved rule revisions and execution history help you understand what happened later.

### Try a rule before allowing it to act

New rules start **Disabled**. **Shadow** mode lets you see matches without changing anything. **Observe** tracks matches for action, but the rule still needs the appropriate permissions, enabled action settings, and arming before it can run after its grace period.

Actions that remove files need destructive arming, including confirmation of how many items currently match. If you change the relevant rule settings, that earlier confirmation no longer covers the new behavior.

A matching rule isn't always allowed to act. A live request retention period can protect a title. Missing or old watch information can hold a watch-based action. A move already working on the same title also takes precedence.

### Combine actions in a sequence

A rule can carry out several actions in order. Scryer records progress through the sequence so recovery can distinguish completed steps from work still to do, including searches that were already accepted.

Rules can work at title, season, or episode level. A rule aimed at one season keeps that selection through execution and recovery.

Jellyfin watch history supports policies based on what has been watched, and storage policies can use available-space information. The supplied templates are starting points: preview their matches and choose their actions before enabling them.

## Request rules and keeping requested titles

**Experimental:** the request-rule editor. Turning off experimental features doesn't cancel existing retention periods or stop rules you've already enabled.

Request rules let you decide which requests can be approved automatically, which need someone to review them, and which should be denied. Rules can consider the library, quality profile, title information, requested retention period, and—when the rule author has permission—the requester.

The request dialog shows the likely outcome as the person makes their choices. If a request needs approval or would be denied, they can see why. A denied request can still be submitted so its outcome and explanation are recorded.

Administrators can see which rules contributed to the decision. Requesters see the result and reasons for their own request. If rules disagree, a denial takes precedence over approval; a rule that fails to evaluate can't approve something by mistake.

### Choose how long to keep a request

A requester can ask to keep a title permanently or for a number of days. **That period starts when the title first imports**, so time spent waiting for approval or downloading doesn't eat into it.

The approver can adjust the period. Managers can extend it later, make it permanent, or release the protection with a recorded reason. Maintenance rules respect active protection, and the “Expired request leases” template can help clean up titles once all their retention periods have ended.

Existing requests keep their previous behavior. They aren't given new expiry dates just because you upgraded.

### Add tags when approving requests

A rule can suggest tags for the approved title, and the approver can review or change them. Those tags need to exist in **Settings → Tags** first. The preview points out unknown tags, and they won't be added to the title.

If you rename a tag, remember to update rules that still use its old name. Renaming the tag doesn't rewrite the rule for you.

Request rules start disabled and can be tried in Shadow mode. Enforcing them also requires the separate request-rule setting. Rules that read information about people require additional permission to manage permissions.

## Organize titles with tags — Standard

Create a shared set of tags in **Settings → Tags**, then use them on movies, series, anime, and series movies. You can pick tags from the registry, change them for several titles at once, and filter your catalog by tag.

Tags also give you a simple way to connect rules: a request rule can label a newly approved title, and a maintenance rule can use that label later. Those rule features are experimental; creating tags and using them to organize your library doesn't require experimental mode.

## Release scores and rule packs — Standard

It's easier to check what your scoring rules will do before relying on them. Choose a real title—and an episode where needed—then preview the score and see what each rule contributed.

You can preview saved rules, search for titles with the keyboard, and choose episodes using the same labels you see elsewhere in the app. Copied community rules are formatted for readability. There's also a prompt helper for preparing a request to an AI assistant when you're writing a rule.

Rule packs can track updates and tell you whether they allow customization. Their summaries are separated from the individual rules, and the rules screens have fewer redundant controls.

### Quality comes before size

Release ranking handles profiles that mix quality tiers more consistently. File size now helps break a tie after the other quality preferences have been considered, so a size preference shouldn't outweigh a better-quality release by itself.

Size preferences account for the codec, and the limits used to reject implausible bitrates are kept separate from preferences for a typical file size. Untouched built-in size settings receive the corresponding correction.

Scoring considers the complete result before deciding whether a penalty leaves a release unacceptable. Releases waiting before download also retain their size-ranking information.

Built-in TRaSH scoring has moved into the shared rule system, with corrections to keep its results consistent. The rule engine also reuses more work when evaluating multiple releases.

## Searching and downloading

**Standard**, except for the standalone **Indexers → Search** tab marked below.

### More consistent automatic searches

Automatic searches and searches you start yourself now follow the same staged approach, including looking for packs first where appropriate. A search stays focused on the titles you selected, and you can cancel while it waits or between download attempts.

You'll get more consistent live feedback about what a search is doing. Search jobs count refused results properly and remember downloads that were already accepted when recovering interrupted work.

A season pack that fills missing episodes can now get past a cooldown intended to prevent unnecessary upgrades to episodes you already own.

### Search an indexer directly — Experimental

The **Search** tab under Indexers lets you search without first adding a title to your library. Send a selected result to a download client or download the release file in your browser. The grab is recorded in history.

### Better handling of indexer problems — Standard

- Testing and saving an indexer now use its assigned proxy, including the checks for supported search features.
- Background checks wait when an indexer is in a retry delay.
- If an indexer asks Scryer to wait before trying again, Scryer respects that minimum delay.
- Successfully validating an indexer's saved settings can clear its retry delay.
- API-key and grab failures show useful errors instead of disappearing or looking like success.
- Supported fallback handling is restored for empty grab responses.
- Reloading a plugin keeps its request tracking and error recording intact.
- Movie searches include the year when the indexer plugin supports it.

### Fewer mix-ups with existing downloads — Standard

A download added outside Scryer stays identified as an external download when Scryer notices it. It no longer becomes Scryer-owned just because it passed through tracking. If you later grab that same download through Scryer, it can take over the existing job without creating a second identity for it.

Imports recover more carefully when a file reached its destination but saving the library record failed. Scryer checks that existing copy before claiming it. If the contents conflict, it leaves the situation for you to resolve.

For SABnzbd and NZBGet downloads with unusable release names, Scryer can look up the original filenames through srrDB. Downloads with a meaningful release name avoid that extra lookup.

This release also corrects which folder an import belongs to, and lets a manually selected file import when its content check has established that it's video.

## Proxies and tunnels — Standard

Set up a proxy once and assign it to the indexers and download clients that should use it. The new Proxies screen includes HTTP and SOCKS options, HTTP/3 proxy support, and SSH and WireGuard tunnels.

You can import a WireGuard configuration and test the outbound IP directly from the screen. SSH tunnels use key authentication, and proxy credentials are stored encrypted.

If an assigned proxy isn't usable, Scryer reports the problem rather than quietly connecting directly. A problem with one assignment doesn't have to stop unrelated indexers from searching.

SOCKS4 isn't offered when creating a new proxy. Existing configuration is retained during upgrade, along with credentials and assignments.

Connection setup has also been tidied up: addresses stay as you type them, presets help fill in names, and provider-specific forms show the relevant options more clearly. Weaver is now the default suggestion when adding a download client.

## More detail about your media — Standard

Scryer can read more information directly from your media files, including video and audio formats, languages, frame rates, aspect ratios, HDR, surround layouts, and subtitles. Imports and library scans use the same analysis, so those details should be more consistent wherever you see them.

Media rows now include surround and HDR labels. There's expanded handling for MKV, MP4, MPEG, ASF/WMV, FLV, and several audio formats.

Large or unusual files get bounded inspection work. If an MKV reaches the read limit, Scryer keeps the useful track information it already found. Timing problems also no longer need to wipe out otherwise readable stream details.

Other fixes improve MP4 frame rates, WAVE channel layouts, AAC bitrate estimates, WMV/ASF information, FLV properties, and MPEG stream details.

### Inspect DVD and Blu-ray titles before importing

Disc images and DVD/Blu-ray structures can contain several titles. You can now inspect those titles in a dedicated dialog before choosing an import.

Your review state is kept as you make selections, and a late response from a title you've left won't replace the one you're currently viewing. Disc-specific controls only appear for disc-backed media.

When Scryer can't fully understand a disc or codec, it reports that instead of filling in a confident-looking answer. Some files may still need review; this doesn't mean every media format is supported.

## Recommendations and artwork — Standard

Personalized discovery has its own setting, enabled by default. You don't need experimental mode to use it.

Recommendations now pay more attention to the mix of **live action, animation, and anime** you already have. A type you don't own is left out of personalized recommendations, and a small part of your library shouldn't dominate every row. The Animation row also sticks to animation.

Genre recommendations need stronger evidence. A single community tag shouldn't be enough to build an unrelated genre row around a show. Results are ordered by relevance and supporting information, with alphabetical order only used as a last tie-breaker.

Scryer won't fill a recommendation row just to make it longer. Rows with fewer than eight qualifying titles are left out. As your library grows, more kinds of recommendation rows become available, and enough growth can bring forward a refresh.

The controls for media-type matching, genre evidence, and recommendation ordering remain available if you'd like to adjust the behavior.

### Clearer search results

Global search shows which libraries already contain a title. When you have it in more than one library, the card keeps the existing title's artwork. Metadata searches cover movies, series, and anime together, and retired title terms are cleaned out of spelling suggestions.

### Artwork work runs overnight

Cached posters and other artwork are optimized overnight, including AVIF encoding and JPEG poster resizing. This keeps the scheduled work away from initial startup. The amount of space saved will depend on the images in your collection.

## Everyday fixes — Standard

The small things add up in this release:

- **Season sizes are more reliable.** Totals no longer depend on which episode rows you've loaded in the browser. Specials show sizes too, and a file covering several episodes isn't counted repeatedly in the same total.
- **Smaller screens get more room.** Indexer settings and Wanted panels adapt better to narrow layouts, keeping labels and buttons readable.
- **Escape takes you back to the title list.** If a dialog or menu is open, it gets to handle Escape first.
- **Large folders are easier to browse.** The folder picker only draws the entries it needs, and an older request can't replace the folder you just opened.
- **Sorting updates the visible list properly.** Rows no longer stay tied to the previous order.
- **Saving library settings clears the unsaved-changes warning.** Switching to a new library also clears a leftover busy state, and saves no longer produce duplicate notifications.
- **Scheduled scans cover every library of the relevant type.** Progress also stays current when scans overlap.
- **Movie overviews show download activity again.** You can also remove blocklist entries from the movie panel again.
- **File lists refresh after deletion finishes.** For files covering several episodes, Scryer updates all the affected episodes and their monitoring state.
- **Emptying the recycle bin runs as a visible background job.** You can follow its progress in the jobs view.
- **Language choices are more consistent.** Audio selections use values the backend recognizes, and metadata languages match what the gateway offers.
- **Title previews are available in more places.** Shared hover cards appear in the calendar and recent dashboard imports. Rename previews can be dismissed.
- **Live connections recover more reliably.** Reconnecting and loading several views at startup do less duplicate work.
- **Settings and navigation have been tidied up.** Selection controls, title settings, sign-in branding, and installed-app icons are more consistent.

**Experimental move screens** also get clearer progress bars, better-spaced actions, and more consistent rows in Activity.

## Administration and integrations — Standard

Library permissions are applied more consistently, including to verification settings. Administrators can edit their own permissions, and viewers without management access no longer see title actions they can't use.

If you use the **API explorer**, it now has a place in system navigation, remembers your drafts, and lets you name tabs. It has its own enable setting and permissions; it doesn't need experimental mode.

For people monitoring Scryer with Prometheus, there are more measurements for indexers, downloads, response handling, freshness, and scoring time. Reading metrics requires an API key with permission to manage system settings.

### If you use scripts or custom integrations

The API has changed to support the new features, so check that your integration supports 0.20.0.

In particular, a script can no longer move an existing title with tracked files simply by changing its root-folder setting. It needs to use the preview-and-move process. Creating a new title with a selected root is still supported.

The experimental switch hides the relevant screens; it isn't a replacement for API permissions or a way to stop previously enabled rules.

## Behind the scenes — Standard

The plugin and rule systems have had substantial updates. Plugins can provide clearer setup forms, and the new plugin host supports indexers, download clients, notifications, and subtitles. Older plugin machinery and the old built-in PAR2 processing path have been retired.

There is also more regression coverage for interrupted moves, preserving files, disconnected storage, import recovery, rule behavior, plugin upgrades, media reading, and release scores. These changes support the features above without adding anything you need to configure.
