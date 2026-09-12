import type { LibraryScanProgress } from "../types/library-scans.ts";
import { isTerminalLibraryScanStatus } from "./library-scan-sessions.ts";

type Snapshot = Record<string, LibraryScanProgress>;

function updatedAt(session: LibraryScanProgress): number {
  return Date.parse(session.updatedAt) || Date.parse(session.startedAt) || 0;
}

/** One freshness gate for subscription snapshots and query reconciliation. */
export class LibraryScanState {
  private snapshot: Snapshot = {};
  private listeners = new Set<() => void>();
  private dismissed = new Set<string>();
  private refreshing: Promise<LibraryScanProgress[]> | null = null;

  getSnapshot = (): Snapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private publish(next: Snapshot) {
    this.snapshot = next;
    for (const listener of this.listeners) listener();
  }

  accept(sessions: LibraryScanProgress[]) {
    const next = { ...this.snapshot };
    for (const incoming of sessions) {
      if (this.dismissed.has(incoming.sessionId)) continue;
      const current = next[incoming.sessionId];
      if (current) {
        // A delayed running snapshot must never revive a completed session.
        if (
          isTerminalLibraryScanStatus(current.status) &&
          !isTerminalLibraryScanStatus(incoming.status)
        )
          continue;
        if (updatedAt(incoming) < updatedAt(current)) continue;
      }
      next[incoming.sessionId] = incoming;
    }
    this.publish(next);
  }

  dismiss(sessionId: string) {
    this.dismissed.add(sessionId);
    if (this.dismissed.size > 256) {
      this.dismissed.delete(this.dismissed.values().next().value!);
    }
    const next = { ...this.snapshot };
    delete next[sessionId];
    this.publish(next);
  }

  refresh(
    fetchActive: () => Promise<LibraryScanProgress[]>,
    fetchSession: (sessionId: string) => Promise<LibraryScanProgress | null>,
  ): Promise<LibraryScanProgress[]> {
    if (this.refreshing) return this.refreshing;
    const startedWith = this.snapshot;
    this.refreshing = (async () => {
      const active = await fetchActive();
      this.accept(active);
      const activeIds = new Set(active.map((session) => session.sessionId));
      // Resolve disappeared sessions individually, with only one request at a
      // time. Absence from the active list is not a completion outcome.
      for (const previous of Object.values(startedWith)) {
        if (
          activeIds.has(previous.sessionId) ||
          isTerminalLibraryScanStatus(previous.status) ||
          this.dismissed.has(previous.sessionId) ||
          isTerminalLibraryScanStatus(
            this.snapshot[previous.sessionId]?.status ?? "RUNNING",
          )
        )
          continue;
        const session = await fetchSession(previous.sessionId);
        if (session) {
          this.accept([session]);
        } else if (this.snapshot[previous.sessionId] === previous) {
          // No longer visible or retained. Do not remove a newer live snapshot.
          const next = { ...this.snapshot };
          delete next[previous.sessionId];
          this.publish(next);
        }
      }
      return Object.values(this.snapshot).filter(
        (session) => !isTerminalLibraryScanStatus(session.status),
      );
    })().finally(() => {
      this.refreshing = null;
    });
    return this.refreshing;
  }

  poll(
    refresh: () => Promise<unknown>,
    onError: (error: unknown) => void,
  ): () => void {
    const timer = setInterval(() => {
      void refresh().catch(onError);
    }, 5_000);
    return () => clearInterval(timer);
  }
}
