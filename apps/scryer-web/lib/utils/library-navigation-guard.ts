import type { Blocker } from "react-router";

export type LibraryNavigationGuardInput = {
  /** The draft differs from the saved library, or is a library not yet created. */
  hasDraftChanges: boolean;
  /** A save is running, from the mutation through the panel adopting its result. */
  saveInFlight: boolean;
};

export function shouldBlockLibraryNavigation({
  hasDraftChanges,
  saveInFlight,
}: LibraryNavigationGuardInput): boolean {
  return hasDraftChanges && !saveInFlight;
}

export type LibraryDiscardPromptInput = {
  hasDraftChanges: boolean;
  blockerState: Blocker["state"];
  pendingLibrarySelection: string | null;
};

/**
 * `open`: a navigation or library switch is waiting on unsaved changes.
 * `settled`: one is waiting, but the changes it waited on are gone.
 * `idle`: nothing is waiting.
 */
export type LibraryDiscardPrompt = "open" | "settled" | "idle";

export function resolveLibraryDiscardPrompt({
  hasDraftChanges,
  blockerState,
  pendingLibrarySelection,
}: LibraryDiscardPromptInput): LibraryDiscardPrompt {
  if (blockerState !== "blocked" && pendingLibrarySelection === null) {
    return "idle";
  }
  // A router blocker stays blocked until it is proceeded or reset, even after
  // the blocker function stops blocking. When the changes are gone (a save
  // landed after the move was requested) there is nothing left to discard.
  return hasDraftChanges ? "open" : "settled";
}
