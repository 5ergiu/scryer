// Mirrors COMPLETED_JOB_TTL_MINUTES in interactive_release_search.rs.
const RESULT_LIFETIME_MS = 5 * 60 * 1000;

export function indexerSearchExpiresAt(snapshot: {
  startedAt?: string;
  completedAt?: string | null;
}): number {
  // Partial results may outlive polling (cancel, disconnect). Using the start
  // time until completion is known expires conservatively in those cases.
  const timestamp = Date.parse(snapshot.completedAt ?? snapshot.startedAt ?? "");
  return Number.isFinite(timestamp) ? timestamp + RESULT_LIFETIME_MS : 0;
}

export function earliestSearchExpiry(deadlines: ReadonlyMap<string, number>): number | null {
  return deadlines.size ? Math.min(...deadlines.values()) : null;
}
