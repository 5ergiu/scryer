/**
 * The keys of the cached media-file maps that list a given file. A deletion
 * targets rows, not files: knowing which episode (or series-movie link) owns
 * the file lets a caller mark exactly those rows pending and hand the same set
 * to the job's terminal handler.
 */
export function mediaFileOwnerKeys(
  fileId: string,
  maps: readonly Record<string, readonly { id: string }[]>[],
): Set<string> {
  const owners = new Set<string>();
  for (const map of maps) {
    for (const [key, files] of Object.entries(map)) {
      if (files.some((file) => file.id === fileId)) {
        owners.add(key);
      }
    }
  }
  return owners;
}

/**
 * The media-file ids a finished deletion run removed. The batch episode-file
 * job reports them as `deletedFileIds`; the single-file job reports the one
 * `fileId` it was asked to delete, which is gone only when the run completed.
 * Returns null when the run carried no usable summary, so the caller can fall
 * back to dropping the whole cached row instead of trusting a partial list.
 */
export function deletedMediaFileIds(run: {
  status: string;
  summaryJson?: unknown;
}): Set<string> | null {
  const parsed =
    typeof run.summaryJson === "string"
      ? (() => {
          try {
            return JSON.parse(run.summaryJson) as unknown;
          } catch {
            return null;
          }
        })()
      : run.summaryJson;
  if (!parsed || typeof parsed !== "object") {
    return null;
  }
  const summary = parsed as { deletedFileIds?: unknown; fileId?: unknown };
  if (summary.deletedFileIds !== undefined) {
    const ids = summary.deletedFileIds;
    if (!Array.isArray(ids) || ids.some((id) => typeof id !== "string")) {
      return null;
    }
    return new Set(ids as string[]);
  }
  if (typeof summary.fileId === "string" && summary.fileId.length > 0) {
    return run.status === "COMPLETED" ? new Set([summary.fileId]) : new Set();
  }
  return null;
}

/**
 * Forget the cached files a deletion run removed. With a known id set only
 * those files leave the cache, so a row's other files (and their badges) stay;
 * without one, every targeted row is dropped so nothing stale is shown.
 */
export function dropDeletedMediaFiles<TFile extends { id: string }>(
  current: Record<string, TFile[]>,
  deletedFileIds: ReadonlySet<string> | null,
  targetedOwnerKeys: ReadonlySet<string>,
): Record<string, TFile[]> {
  if (deletedFileIds) {
    return Object.fromEntries(
      Object.entries(current).map(([key, files]) => [
        key,
        files.filter((file) => !deletedFileIds.has(file.id)),
      ]),
    );
  }
  return Object.fromEntries(
    Object.entries(current).filter(([key]) => !targetedOwnerKeys.has(key)),
  );
}
