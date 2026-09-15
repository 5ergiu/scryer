export type StatusToastKind = "SUCCESS" | "ERROR" | "WARNING";

const RENAME_APPLY_COMPLETE_PATTERN =
  /\brename apply complete:\s*(\d+)\s+applied,\s*(\d+)\s+skipped,\s*(\d+)\s+failed\b/i;

const NO_TOAST_PATTERNS: RegExp[] = [
  /^ready\.?$/i,
  /\bsearching\b/i,
  /\bsearching\s+(?:tvdb|nzb)\b/i,
  /\bfound\s+\d+\s+(?:tvdb|nzb)\b/i,
  /\bfound\s+\d+\s+nzb\s+item/i,
  /\bselected tvdb match/i,
  /\btvdb queue tip/i,
  /\bnzb queue tip/i,
  /\bno nzb results?\b/i,
  /\bno results found\b/i,
  /\bshowing activity stream\b/i,
  /\bhiding activity stream\b/i,
  /\bediting\s+(?:user|indexer|download client)\b/i,
  /\bdelete\s+(?:user|indexer|download client)\?/i,
  /\blanguage set to/i,
  /\bfacet is required\b/i,
  /\btitle is required\b/i,
  /\busername and password are required\b/i,
  /\bpassword is required\b/i,
  /\btesting nzbget connection/i,
];

const ERROR_PATTERNS: RegExp[] = [
  /^search results expired, please search again$/i,
  /\bfailed to\b/i,
  /\bfailed\b/i,
  /\brequest failed\b/i,
  /\berror\b/i,
  /\bvalidation:\s*/i,
  /\bfailed to (?:load|save|update|create|delete|queue|connect|connect\s+to)\b/i,
  /\bqueue operation failed\b/i,
  /\bdownload client connection test failed\b/i,
  /\bno download client enabled\b/i,
  /\binvalid\b/i,
];

const WARNING_PATTERNS: RegExp[] = [
  // An acquisition the canonical-submission guard deferred: an earlier download
  // on the same scope is still awaiting lifecycle reconciliation. The request
  // is retried on its own, and no download client is down, so this is a
  // transient hold rather than a failure — amber, not red. Listed here because
  // warnings are matched before errors and the surrounding sentence a caller
  // wraps it in ("failed to queue: …") would otherwise read as an error.
  /\bawaiting lifecycle reconciliation\b/i,
  /\bacquisition deferred\b/i,
  /\bskipped\b/i,
  /\bblocked by quality profile\b/i,
  /\bblocked by policy\b/i,
  /\bblocked by quality\b/i,
  /\bno source to queue\b/i,
  /\bno release found\b/i,
  /\bno nzb result found\b/i,
  /\bno usable imdb id\b/i,
  /\bno valid tvdb id\b/i,
  /\bno searchable title\b/i,
  /\bunknown quality profile id\b/i,
  /\bno source\b/i,
];

const SUCCESS_PATTERNS: RegExp[] = [
  /\badded\b.*\bcatalog/i,
  /\bqueued\b/i,
  /\brename apply complete\b/i,
  /\brename preview ready\b/i,
  /\bsaved\b/i,
  /\bupdated\b/i,
  /\bcreated\b/i,
  /\bdeleted\b/i,
  /\bapplied\b/i,
  /\bpassed\b/i,
  /\bcomplete\b/i,
  /\bimported\b/i,
];

export function classifyStatusToastLevel(message: string): StatusToastKind | null {
  const normalized = message.trim().toLowerCase();
  if (!normalized) {
    return null;
  }

  const renameApplyMatch = normalized.match(RENAME_APPLY_COMPLETE_PATTERN);
  if (renameApplyMatch) {
    const skipped = Number.parseInt(renameApplyMatch[2] ?? "0", 10);
    const failed = Number.parseInt(renameApplyMatch[3] ?? "0", 10);
    if (failed > 0) {
      return "ERROR";
    }
    if (skipped > 0) {
      return "WARNING";
    }
    return "SUCCESS";
  }

  if (NO_TOAST_PATTERNS.some((pattern) => pattern.test(normalized))) {
    return null;
  }

  if (WARNING_PATTERNS.some((pattern) => pattern.test(normalized))) {
    return "WARNING";
  }

  if (ERROR_PATTERNS.some((pattern) => pattern.test(normalized))) {
    return "ERROR";
  }

  if (SUCCESS_PATTERNS.some((pattern) => pattern.test(normalized))) {
    return "SUCCESS";
  }

  return null;
}

/**
 * The level a status toasts at: the caller's explicit level when it gave one,
 * otherwise whatever the wording classifies as. An empty status never toasts.
 */
export function resolveStatusToastLevel(
  message: string,
  options?: { level?: StatusToastKind },
): StatusToastKind | null {
  if (!message.trim()) {
    return null;
  }
  return options?.level ?? classifyStatusToastLevel(message);
}
