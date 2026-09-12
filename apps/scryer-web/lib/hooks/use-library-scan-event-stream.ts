import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";
import { useClient } from "urql";

import {
  activeLibraryScansQuery,
  libraryScanSessionQuery,
  libraryScanStateSubscriptionQuery,
} from "@/lib/graphql/queries";
import { useDeferredWsSubscription } from "@/lib/hooks/use-deferred-ws-subscription";
import type { Facet, LibraryScanProgress } from "@/lib/types";
import { normalizeLibraryScanProgress } from "@/lib/utils/job-runs";
import {
  findActiveLibraryScanSession,
  isTerminalLibraryScanStatus,
} from "@/lib/utils/library-scan-sessions";
import { LibraryScanState } from "@/lib/utils/library-scan-state";

export function useLibraryScanEventStream() {
  const client = useClient();
  const [state] = useState(() => new LibraryScanState());
  const sessionsById = useSyncExternalStore(
    state.subscribe,
    state.getSnapshot,
    state.getSnapshot,
  );

  const refreshSessions = useCallback(
    () =>
      state.refresh(
        async () => {
          const { data, error } = await client
            .query(
              activeLibraryScansQuery,
              {},
              { requestPolicy: "network-only" },
            )
            .toPromise();

          if (error) {
            throw error;
          }

          const rawSessions: unknown[] = Array.isArray(data?.activeLibraryScans)
            ? data.activeLibraryScans
            : [];
          const sessions = rawSessions
            .map(normalizeLibraryScanProgress)
            .filter(
              (session): session is LibraryScanProgress => session !== null,
            );

          return sessions;
        },
        async (sessionId) => {
          const { data, error } = await client
            .query(
              libraryScanSessionQuery,
              { sessionId },
              { requestPolicy: "network-only" },
            )
            .toPromise();
          if (error) throw error;
          return normalizeLibraryScanProgress(data?.libraryScanSession);
        },
      ),
    [client, state],
  );

  useEffect(() => {
    let cancelled = false;

    void refreshSessions().catch((error) => {
      if (cancelled) {
        return;
      }

      console.error(
        "[library-scan-events] failed to bootstrap active scan sessions:",
        error,
      );
    });

    return () => {
      cancelled = true;
    };
  }, [refreshSessions]);

  useDeferredWsSubscription<{ data?: { libraryScanState?: unknown } }>({
    enabled: true,
    requestKey: "libraryScanState",
    request: {
      query: libraryScanStateSubscriptionQuery,
      variables: {},
    },
    onNext(result) {
      const session = normalizeLibraryScanProgress(
        result.data?.libraryScanState,
      );
      if (!session) {
        return;
      }

      state.accept([session]);
    },
    onError(error) {
      console.error("[library-scan-events] subscription error:", error);
    },
  });

  const dismissSession = useCallback(
    (sessionId: string) => {
      state.dismiss(sessionId);
    },
    [state],
  );

  const sessions = useMemo(
    () =>
      Object.values(sessionsById)
        .filter((session) => session.mode === "FULL")
        .sort((left, right) => left.startedAt.localeCompare(right.startedAt)),
    [sessionsById],
  );

  const hasActiveSessions = sessions.some(
    (session) => !isTerminalLibraryScanStatus(session.status),
  );
  useEffect(() => {
    if (!hasActiveSessions) {
      return;
    }
    return state.poll(refreshSessions, (error) => {
      console.error(
        "[library-scan-events] failed to reconcile active scan sessions:",
        error,
      );
    });
  }, [refreshSessions, hasActiveSessions, state]);

  const getActiveSession = useCallback(
    (facet: Facet, libraryId?: string | null) =>
      findActiveLibraryScanSession(sessions, facet, libraryId),
    [sessions],
  );

  return {
    sessions,
    getActiveSession,
    refreshSessions,
    dismissSession,
  };
}
