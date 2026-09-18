# Scryer 0.21.3 release notes

These notes cover what's changed since **0.21.1**.

## Highlights

- **Scryer now has a desktop app on macOS.** Each release publishes a disk image per architecture: `scryer-darwin-arm64.dmg` for Apple silicon and `scryer-darwin-x86_64.dmg` for Intel. Open it and drag **Scryer** to Applications.
  - Scryer lives in the menu bar. It starts the server for you and opens the web UI in its own window, so there is no browser tab to keep open. Clicking the menu-bar icon shows Scryer's status and the usual actions.
  - Scryer.app is **not signed with an Apple Developer ID and is not notarized**, so the first launch needs one extra step. On macOS 15 and later, open Scryer once and dismiss the warning, then go to **System Settings → Privacy & Security** and click **Open Anyway** next to the message about Scryer. On macOS 14 and earlier you can instead right-click (or Control-click) Scryer in Applications and choose **Open**, then **Open** again. You only do this once.
  - If you would rather clear the quarantine flag from a terminal, run `xattr -dr com.apple.quarantine /Applications/Scryer.app`.
  - The `scryer-darwin-*-portable.tar.gz` archives still ship the plain `scryer` server for anyone running it headless.

- **The Windows installer now installs a desktop app.** Launching Scryer from the Start menu puts an icon in the notification area, starts the Scryer server in the background, and opens the web UI in an embedded Microsoft Edge WebView2 window. There is no browser tab and no service to configure.
  - Clicking the tray icon shows Scryer's status and the usual actions: open, restart and quit. You can also choose to start Scryer when you sign in.
  - If WebView2 is not installed, Scryer opens the web UI in your default browser instead.
  - If the server fails to start, the app now shows the error the server reported instead of waiting and then timing out.
  - Existing MSI installs upgrade in place and keep their settings, including whether Scryer starts when you sign in.
  - **Uninstalling now removes Scryer's data.** See [Upgrading](#upgrading) before you uninstall.

- **In-app upgrades now work for the macOS app.** When a new version is out, Scryer downloads the replacement app, checks its signature before anything moves, swaps it for the installed one and reopens itself on the new version. The previous version is kept beside it until the new one has started.
  - For this to work, Scryer has to be able to replace the app where it lives. Drag it into your **Applications** folder (or any folder you can write to) and open it from there.
  - If you are running Scryer straight from the disk image, or from the temporary copy macOS makes when an app is opened from your Downloads folder, the upgrade page tells you to move it to Applications instead of upgrading.
  - Older versions of Scryer keep finding and installing new releases as before.

- **Importing from Sonarr and Radarr handles very large libraries.** Finishing the import wizard used to do all of its work in one request. On a large catalog the button faded and the page never moved, and trying again got stuck behind the first attempt.
  - Finishing the import now runs in the background. The Summary step shows its progress, keeps running if you leave the page, and picks up where it was if you reload.
  - If finishing fails, the error stays on the page and **Finish** becomes available again so you can retry.
  - The import itself is much faster. A test catalog of 2,000 series with 20 episodes each and 2,000 movies went from more than three minutes to under half a second, and a catalog of 100,000 series and 100,000 movies finishes in about 20 seconds.

- **Scryer does much less work while downloads are running.** Several steady sources of database load have been removed, which matters most on large libraries and busy download clients.
  - Items in your download client's queue and history that haven't changed are no longer looked up again on every poll. Before, a history of 100 downloads Scryer never sent could cost hundreds of database writes every 10 seconds, indefinitely.
  - Download history and queue lookups use new indexes instead of scanning.
  - The counts on the navigation menu are now kept up to date in the background instead of being recalculated by every open browser tab every 30 seconds.
  - Title lists read by background tasks (matching, RSS, subtitles and folder checks) no longer load every title's tags, which took about a second per read on a library of a couple of thousand titles.

## Included fixes

- **Manual import:** a failed manual import is now shown, with its reason. Before, history showed a failed entry with no message, and the download in the queue went straight back to its original "blocked" message, so there was no sign anything had been tried. The queue now keeps the failed result, including the source and destination paths when a file failed. When files were moved but the download could not be confirmed, the queue says so instead of repeating the old block reason.
- **Manual import:** the activity page, the dashboard and a title's page now always agree on which downloads can be imported by hand, and Scryer refuses a manual import it would not have offered.
- **Manual import:** manually importing a series or anime now writes the series `.nfo` file when writing `.nfo` files is turned on, as automatic imports already did.
- **NFO files:** a `.nfo` file that fails partway through being written is no longer left behind half-written. Before, a later import could mistake the partial file for one you had written yourself and never replace it.
- **Import:** on a download client with a very busy history, completed downloads from a quieter download client could be crowded out of the list Scryer checks for imports. Each client's most recent completions are now always considered.
- **Library scan:** a folder tagged with an id, such as `Example Film (2015) {tmdb-12345}`, could be matched to a completely different title when the metadata service did not know that id yet, often producing a "Title already owns another folder" pending import. Scryer now only looks the folder up by its id. If the id cannot be found yet, the folder is left as a pending import that says so, and the next scan tries again. An id in a folder name also now wins over a title read from a file name inside it.
- **Download clients:** Scryer no longer adopts downloads it did not send when they sit in a category Scryer does not use. Before, sharing a download client with your own downloads (a separate music category, for example) left Scryer tracking every one of them indefinitely. Downloads Scryer did not send are also released once they leave the client. Clients with no categories set, and downloads that report no category, are handled as before.
- **Weaver:** a job in one of the newer Weaver states (fetching repair data, finalizing, post-processing) no longer breaks Scryer's view of the whole Weaver queue. Before, a single job in one of those states could make Scryer lose track of every Weaver download and send titles again that were already downloading.
- **Weaver:** Scryer no longer cancels a running Weaver download because its own information about the job was out of date. The download keeps running and the disagreement is logged instead.
- **Notifications:** a notification channel subscribed to **File Deleted** no longer also receives **File Deleted For Upgrade**. Each event now reaches only the channels subscribed to it. This addresses issue #227.
- **Release names:** repost tags that uploaders add after the real release group, such as `-AsRequested`, `-Obfuscated` or `-Scrambled`, are no longer mistaken for the release group.
- **Media info:** when an audio track has no language set, Scryer now shows the language it worked out from the track's name, in italics with an asterisk and a note explaining where it came from. This appears in the **Info** window and in the audio badge's pop-up.
- **Windows:** uninstalling or upgrading Scryer can no longer hang on an error window that nobody can see.
- **Diagnostics:** Scryer now logs a separate warning when the server itself is running behind and when all of its database connections are in use, so slow-database warnings in the logs can be told apart.

## API changes

These affect scripts and tools that call Scryer's GraphQL API. The Scryer web app is already updated.

- **Changed:** `finalizeExternalImport` now returns as soon as the import has been checked and started, before it is applied. It returns a new `finalizeSessionId` and the starting `progress`. Poll `externalImportWarmupStatus` with that id until it reaches a finished status. Validation errors are still returned by `finalizeExternalImport` itself.
- **Added:** download queue items have an `importActions` field that says which import actions the item currently offers: `manualImportInteractive`, `manualImportDirect`, `assignTitle`, `ignore` and `markFailed`.
- **Added:** audio streams have an `inferredLanguage` field, set when the track has no language of its own and one could be worked out from its name.
- **Added:** the installation kind enum `ApplicationInstallationKindValue` has a new value, `MACOS_APP_BUNDLE`. Clients that match every enum value should handle it.

## Upgrading

Scryer updates its database automatically the first time the new version starts. No configuration changes are required.

**Windows: uninstalling now deletes your Scryer data. Back up first.** In earlier versions your data stayed on disk after uninstalling. From this version, uninstalling deletes Scryer's data folder, `%LOCALAPPDATA%\ScryerMedia\Scryer`, which holds the database, logs, settings and the WebView2 profile, and removes Scryer's Windows Credential Manager entry. A `backups` folder inside it is kept if it still contains anything. Your media libraries and download folders are outside that folder and are never touched. Upgrading, whether from inside Scryer, from a new MSI or through winget, keeps everything. If you might want your library back after uninstalling, take a backup and copy it somewhere else first.

**SSH tunnel proxies:** an SSH server used as a proxy must now offer an Ed25519 host key, and support the `curve25519-sha256` key exchange. A server that offers only an ECDSA host key, or only older key exchanges, no longer connects. Most current OpenSSH servers already offer both.

**Verifying release signatures:** container images are now signed with Cosign v3, which stores the signature in a different place, so verify them with Cosign v3. Downloaded files keep their existing `.sigstore.json` signature, and each now also has a `.cosign-v3.sigstore.json` signature beside it for Cosign v3.
