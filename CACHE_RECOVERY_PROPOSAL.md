# Expired Wasmtime cleanup marker recovery — proposal only

No recovery or file-removal behavior is implemented by this change. Approval of
this proposal must precede any deletion-path implementation. Operating on a
production cache requires separate authorization for that exact operation.

## Confirmed behavior

Wasmtime 48.0.1's cache worker ignores expired markers when checking whether a
cleanup task is already owned, then uses `create_new` on
`.cleanup.wip-<current PID>`. An expired marker from an earlier process with the
same PID still occupies that filename, so acquisition fails with AlreadyExists.
Cleanup cannot run to remove its own stale marker. PID reuse is common across
container restarts. This affects maintenance, while compilation and artifact
reuse can continue.

The regression `expired_same_pid_cleanup_lock_does_not_prevent_cache_reuse`
creates a preserved synthetic cache with the current PID marker dated one hour
after the Unix epoch. It compiles a module, verifies a fresh engine gets a cache
hit with zero misses, and confirms the expired filename still collides. The
fixture path is printed by the test. It does not exercise recovery.

## Proposed boundary

Limit recovery to startup, before either Wasmtime engine or cache worker exists.
The only candidate is the exact `.cleanup.wip-<current PID>` directly inside the
configured Scryer Wasmtime cache directory. Do not glob `.wip-*`, recover other
task markers, traverse descendants, or touch compiled modules or cache stats.

Require all of the following before changing that single marker:

1. Establish exclusive ownership of the cache directory for the complete engine
   lifetime. An application-owned OS lease must cover every supported Scryer
   process sharing that directory. If an older process, another container or an
   external Wasmtime user could share it without honoring that lease, refuse
   automatic recovery. A PID alone is not an ownership proof across namespaces.
2. Open the directory without following symlinks and use a directory-relative
   operation. Reject a symlink directory, symlink marker, nonregular marker,
   unexpected owner, multiple hard links, nonzero marker content, or metadata
   that cannot be read. Do not resolve an alternate path and then remove by name.
3. Match Wasmtime's installed cleanup expiration interval and allowed clock
   drift. Require a readable timestamp older than both the expiration threshold
   and this process's start time. Future timestamps or uncertain clocks fail
   closed. Age alone never proves abandonment.
4. Exclude any active owner. The current PID exception is valid only before this
   process has initialized a cache worker, with exclusive ownership established.
   Never use a generic process-liveness check to justify removing a marker owned
   by an unrelated PID or another container incarnation.
5. Recheck the directory identity, marker inode/file identity, owner, size and
   timestamp immediately before the operation under the lease. Refuse recovery
   if anything changed. If the platform cannot enforce the ownership and race
   boundary, preserve the marker and report the limitation.

Only after those conditions are implementable and explicitly approved should
the single stale marker be removed. Do not suppress warnings, disable caching,
purge artifacts, change Wasmtime versions, or broaden this into routine cleanup.
Failure to recover must preserve normal compilation and cache reuse.

## Required validation for a future implementation

Use synthetic fixtures for an expired same-PID marker, unexpired markers, other
PIDs, active workers, concurrent startups, PID namespace ambiguity, symlink
directories and markers, hard links, future timestamps, permission failures,
metadata replacement races, and cancellation. Assert compiled artifacts,
unrelated files and every rejected marker retain their bytes and identity.
Prove exactly one startup can recover, and that the next worker can acquire
cleanup ownership. Keep fixtures and evidence; never test against user files.
