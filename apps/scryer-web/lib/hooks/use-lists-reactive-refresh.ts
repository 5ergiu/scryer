import { useEffect, useId, useRef } from "react";

import { useReactiveRefresh } from "@/lib/context/reactive-refresh-context";
import type { DomainEventPredicate } from "@/lib/reactive/domain-event-feed";

/**
 * Calls `onChanged` whenever a live event matching `changed` arrives: one of
 * the list predicates in `@/lib/utils/lists`, naming what the surface shows.
 * The change may have been made here or in another session. The event carries
 * nothing to apply; the surface reads its data again.
 *
 * Several events in a burst (a "sync all") arrive as one call. `changed` must
 * be stable across renders.
 */
export function useListsReactiveRefresh(
  changed: DomainEventPredicate,
  onChanged: () => void,
  options?: { pause?: boolean },
) {
  const { registerReactiveRefresh } = useReactiveRefresh();
  const onChangedRef = useRef(onChanged);
  useEffect(() => {
    onChangedRef.current = onChanged;
  });

  const pause = options?.pause ?? false;
  const aliasId = useId();

  useEffect(() => {
    if (pause) {
      return;
    }
    return registerReactiveRefresh({
      aliasKey: `lists:${aliasId}`,
      predicate: changed,
      run: () => onChangedRef.current(),
    });
  }, [changed, pause, aliasId, registerReactiveRefresh]);
}
